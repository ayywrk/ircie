pub mod events;
pub mod factory;
pub mod format;
pub mod irc_command;
pub mod system;
pub mod system_params;
pub mod utils;

use std::{
    any::TypeId,
    collections::{HashMap, HashSet, VecDeque},
    io::ErrorKind,
    net::ToSocketAddrs,
    path::Path,
    sync::Arc,
    time::{Duration, SystemTime},
};

use async_native_tls::TlsStream;
use factory::Factory;
use irc_command::IrcCommand;
use log::{debug, info, trace, warn};
use serde::{Deserialize, Serialize};
use system::{IntoSystem, Response, StoredSystem, System};
use tokio::{
    fs::File,
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadHalf, WriteHalf},
    net::TcpStream,
    sync::{mpsc, RwLock},
};

pub(crate) const MAX_MSG_LEN: usize = 512;

#[derive(Default)]
pub enum Stream {
    Plain(TcpStream),
    Tls(TlsStream<TcpStream>),
    #[default]
    None,
}

impl Stream {
    pub async fn read(&mut self, buf: &mut [u8]) -> std::result::Result<usize, std::io::Error> {
        match self {
            Stream::Plain(stream) => stream.read(buf).await,
            Stream::Tls(stream) => stream.read(buf).await,
            Stream::None => panic!("No stream."),
        }
    }

    pub async fn write(&mut self, buf: &[u8]) -> std::result::Result<usize, std::io::Error> {
        match self {
            Stream::Plain(stream) => stream.write(buf).await,
            Stream::Tls(stream) => stream.write(buf).await,
            Stream::None => panic!("No stream."),
        }
    }
}

pub struct FloodControl {
    last_cmd: SystemTime,
}

impl Default for FloodControl {
    fn default() -> Self {
        Self {
            last_cmd: SystemTime::now(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct IrcPrefix<'a> {
    pub nick: &'a str,
    pub user: Option<&'a str>,
    pub host: Option<&'a str>,
}

impl<'a> From<&'a str> for IrcPrefix<'a> {
    fn from(prefix_str: &'a str) -> Self {
        let prefix_str = &prefix_str[1..];

        let nick_split: Vec<&str> = prefix_str.split('!').collect();
        let nick = nick_split[0];

        // we only have a nick
        if nick_split.len() == 1 {
            return Self {
                nick,
                ..Default::default()
            };
        }

        let user_split: Vec<&str> = nick_split[1].split('@').collect();
        let user = user_split[0];

        // we don't have an host
        if user_split.len() == 1 {
            return Self {
                nick: nick,
                user: Some(user),
                ..Default::default()
            };
        }

        Self {
            nick: nick,
            user: Some(user),
            host: Some(user_split[1]),
        }
    }
}

pub struct IrcMessage<'a> {
    prefix: Option<IrcPrefix<'a>>,
    command: IrcCommand,
    parameters: Vec<&'a str>,
}

impl<'a> From<&'a str> for IrcMessage<'a> {
    fn from(line: &'a str) -> Self {
        let mut elements = line.split_whitespace();

        let tmp = elements.next().unwrap();

        if tmp.chars().next().unwrap() == ':' {
            return Self {
                prefix: Some(tmp.into()),
                command: elements.next().unwrap().into(),
                parameters: elements.collect(),
            };
        }

        Self {
            prefix: None,
            command: tmp.into(),
            parameters: elements.collect(),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct IrcConfig {
    host: String,
    port: u16,
    ssl: bool,
    channels: HashSet<String>,
    nick: String,
    user: String,
    real: String,
    nickserv_pass: Option<String>,
    nickserv_email: Option<String>,
    cmdkey: String,
    flood_interval: f32,
    owner: String,
    admins: Vec<String>,
}

// TODO:
/*
   split Irc into two structs, one for the context, which is Send + Sync to be usable in tasks
   one for the comms.

*/

pub struct Context {
    config: IrcConfig,
    identified: bool,
    send_queue: VecDeque<String>,

    systems: HashMap<String, StoredSystem>,
    interval_tasks: Vec<(Duration, StoredSystem)>,
    factory: Arc<RwLock<Factory>>,
}

impl Context {
    pub fn privmsg(&mut self, channel: &str, message: &str) {
        debug!("sending privmsg to {} : {}", channel, message);
        self.queue(&format!("PRIVMSG {} :{}", channel, message));
    }
    fn queue(&mut self, msg: &str) {
        let mut msg = msg.replace("\r", "").replace("\n", "");

        if msg.len() > MAX_MSG_LEN - "\r\n".len() {
            let mut i = 0;

            while i < msg.len() {
                let max = (MAX_MSG_LEN - "\r\n".len()).min(msg[i..].len());

                let mut m = msg[i..(i + max)].to_owned();
                m = m + "\r\n";
                self.send_queue.push_back(m);
                i += MAX_MSG_LEN - "\r\n".len()
            }
        } else {
            msg = msg + "\r\n";
            self.send_queue.push_back(msg);
        }
    }

    pub fn identify(&mut self) {
        if self.config.nickserv_pass.is_none() || self.identified {
            return;
        }

        self.privmsg(
            "NickServ",
            &format!("IDENTIFY {}", self.config.nickserv_pass.as_ref().unwrap()),
        );
    }

    pub fn register(&mut self) {
        info!(
            "Registering as {}!{} ({})",
            self.config.nick, self.config.user, self.config.real
        );
        self.queue(&format!(
            "USER {} 0 * {}",
            self.config.user, self.config.real
        ));
        self.queue(&format!("NICK {}", self.config.nick));
    }

    fn is_owner(&self, prefix: &IrcPrefix) -> bool {
        self.is_admin(prefix, &self.config.owner)
    }

    fn is_admin(&self, prefix: &IrcPrefix, admin: &str) -> bool {
        let admin = ":".to_owned() + &admin;
        let admin_prefix: IrcPrefix = admin.as_str().into();

        if (admin_prefix.nick == prefix.nick || admin_prefix.nick == "*")
            && (admin_prefix.user == prefix.user || admin_prefix.user == Some("*"))
            && (admin_prefix.host == prefix.host || admin_prefix.host == Some("*"))
        {
            return true;
        }

        false
    }

    fn join(&mut self, channel: &str) {
        info!("Joining {channel}");
        self.queue(&format!("JOIN {}", channel));
        self.config.channels.insert(channel.to_owned());
    }

    fn join_config_channels(&mut self) {
        for i in 0..self.config.channels.len() {
            let channel = self.config.channels.iter().nth(i).unwrap();
            info!("Joining {channel}");
            self.queue(&format!("JOIN {}", channel))
        }
    }

    fn update_nick(&mut self, new_nick: &str) {
        self.config.nick = new_nick.to_owned();
        self.queue(&format!("NICK {}", self.config.nick));
    }

    pub fn privmsg_all(&mut self, message: &str) {
        for i in 0..self.config.channels.len() {
            let channel = self.config.channels.iter().nth(i).unwrap();
            debug!("sending privmsg to {} : {}", channel, message);
            self.queue(&format!("PRIVMSG {} :{}", channel, message));
        }
    }

    pub fn add_system<I, S: for<'a> System<'a> + Send + Sync + 'static>(
        &mut self,
        name: &str,
        system: impl for<'a> IntoSystem<'a, I, System = S>,
    ) -> &mut Self {
        self.systems
            .insert(name.to_owned(), Box::new(system.into_system()));
        self
    }

    pub fn add_interval_task<I, S: for<'a> System<'a> + Send + Sync + 'static>(
        &mut self,
        duration: Duration,
        system_task: impl for<'a> IntoSystem<'a, I, System = S>,
    ) -> &mut Self {
        self.interval_tasks
            .push((duration, Box::new(system_task.into_system())));
        self
    }

    pub async fn add_resource<R: Send + Sync + 'static>(&mut self, res: R) -> &mut Self {
        self.factory
            .write()
            .await
            .resources
            .insert(TypeId::of::<R>(), Box::new(res));
        self
    }

    pub async fn run_system<'a>(&mut self, prefix: &'a IrcPrefix<'a>, name: &str) -> Response {
        let system = self.systems.get_mut(name).unwrap();
        system.run(prefix, &mut *self.factory.write().await)
    }

    pub async fn run_interval_tasks(&mut self, tx: mpsc::Sender<Vec<String>>) {
        for (task_duration, mut task) in std::mem::take(&mut self.interval_tasks) {
            let fact = self.factory.clone();
            let task_tx = tx.clone();
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(task_duration).await;
                    let resp = task.run(
                        &IrcPrefix {
                            nick: "",
                            user: None,
                            host: None,
                        },
                        &mut *fact.write().await,
                    );
                    if resp.0.is_none() {
                        continue;
                    }
                    task_tx.send(resp.0.unwrap()).await.unwrap()
                }
            });
        }
    }
}

pub trait AsyncReadWrite: AsyncRead + AsyncWrite + Send + Unpin {}

impl<T: AsyncRead + AsyncWrite + Send + Unpin> AsyncReadWrite for T {}

pub struct Irc {
    context: Arc<RwLock<Context>>,
    flood_controls: HashMap<String, FloodControl>,
    stream: Option<Box<dyn AsyncReadWrite>>,
    partial_line: String,
}

impl Irc {
    pub async fn from_config(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let mut file = File::open(path).await?;
        let mut contents = String::new();
        file.read_to_string(&mut contents).await?;

        let config: IrcConfig = serde_yaml::from_str(&contents).unwrap();

        let context = Arc::new(RwLock::new(Context {
            config,
            identified: false,
            send_queue: VecDeque::new(),
            systems: HashMap::default(),
            interval_tasks: Vec::new(),
            factory: Arc::new(RwLock::new(Factory::default())),
        }));

        Ok(Self {
            context,
            stream: None,
            flood_controls: HashMap::default(),
            partial_line: String::new(),
        })
    }

    pub async fn add_system<I, S: for<'a> System<'a> + Send + Sync + 'static>(
        &mut self,
        name: &str,
        system: impl for<'a> IntoSystem<'a, I, System = S>,
    ) -> &mut Self {
        {
            let mut context = self.context.write().await;
            context.add_system(name, system);
        }
        self
    }

    pub async fn add_interval_task<I, S: for<'a> System<'a> + Send + Sync + 'static>(
        &mut self,
        duration: Duration,
        system: impl for<'a> IntoSystem<'a, I, System = S>,
    ) -> &mut Self {
        {
            let mut context = self.context.write().await;
            context.add_interval_task(duration, system);
        }
        self
    }

    pub async fn add_resource<R: Send + Sync + 'static>(&mut self, res: R) -> &mut Self {
        {
            let mut context = self.context.write().await;
            context.add_resource(res).await;
        }
        self
    }

    pub async fn connect(&mut self) -> std::io::Result<()> {
        let context = self.context.read().await;

        let domain = format!("{}:{}", context.config.host, context.config.port);

        info!("Connecting to {}", domain);

        let mut addrs = domain
            .to_socket_addrs()
            .expect("Unable to get addrs from domain {domain}");

        let sock = addrs
            .next()
            .expect("Unable to get ip from addrs: {addrs:?}");

        let plain_stream = TcpStream::connect(sock).await?;

        if context.config.ssl {
            let stream = async_native_tls::connect(context.config.host.clone(), plain_stream)
                .await
                .unwrap();
            self.stream = Some(Box::new(stream));
            return Ok(());
        }

        self.stream = Some(Box::new(plain_stream));
        Ok(())
    }

    async fn is_flood(&mut self, channel: &str) -> bool {
        let mut flood_control = match self.flood_controls.entry(channel.to_owned()) {
            std::collections::hash_map::Entry::Occupied(o) => o.into_mut(),
            std::collections::hash_map::Entry::Vacant(v) => {
                v.insert(FloodControl {
                    last_cmd: SystemTime::now(),
                });
                return false;
            }
        };

        let elapsed = flood_control.last_cmd.elapsed().unwrap();

        if elapsed.as_secs_f32() < self.context.read().await.config.flood_interval {
            warn!("they be floodin @ {channel}!");
            return true;
        }

        flood_control.last_cmd = SystemTime::now();
        false
    }

    pub async fn handle_commands(&mut self, mut lines: VecDeque<String>) {
        while lines.len() != 0 {
            let owned_line = lines.pop_front().unwrap();
            let line = owned_line.as_str();

            trace!("<< {:?}", line);

            let message: IrcMessage = line.into();
            self.handle_message(&message).await;
        }
    }

    async fn handle_message<'a>(&mut self, message: &'a IrcMessage<'a>) {
        match message.command {
            IrcCommand::PING => self.event_ping(&message.parameters[0]).await,
            IrcCommand::RPL_WELCOME => self.event_welcome(&message.parameters[1..].join(" ")).await,
            IrcCommand::ERR_NICKNAMEINUSE => self.event_nicknameinuse().await,
            IrcCommand::KICK => {
                self.event_kick(
                    &message.parameters[0],
                    &message.parameters[1],
                    &message.prefix.as_ref().unwrap().nick,
                    &message.parameters[2..].join(" "),
                )
                .await
            }
            IrcCommand::QUIT => self.event_quit(message.prefix.as_ref().unwrap()).await,
            IrcCommand::INVITE => {
                self.event_invite(
                    message.prefix.as_ref().unwrap(),
                    &message.parameters[1][1..],
                )
                .await
            }
            IrcCommand::PRIVMSG => {
                self.event_privmsg(
                    message.prefix.as_ref().unwrap(),
                    &message.parameters[0],
                    &message.parameters[1..].join(" ")[1..],
                )
                .await
            }
            IrcCommand::NOTICE => {
                self.event_notice(
                    message.prefix.as_ref(),
                    &message.parameters[0],
                    &message.parameters[1..].join(" ")[1..],
                )
                .await
            }
            _ => {}
        }
    }

    pub async fn run(&mut self) -> std::io::Result<()> {
        self.connect().await?;
        info!("Ready!");
        let (tx, mut rx) = mpsc::channel::<Vec<String>>(512);
        {
            let mut context = self.context.write().await;
            context.register();
            context.run_interval_tasks(tx).await;
        }

        let stream = self.stream.take().unwrap();

        let (mut reader, mut writer) = tokio::io::split(stream);

        let cloned_ctx = self.context.clone();
        tokio::spawn(async move {
            loop {
                handle_rx(&mut rx, &cloned_ctx).await;
            }
        });

        let cloned_ctx = self.context.clone();
        tokio::spawn(async move {
            loop {
                send(&mut writer, &cloned_ctx).await.unwrap();
            }
        });

        loop {
            let lines = recv(&mut reader, &mut self.partial_line).await?;
            self.handle_commands(lines.into()).await;
        }
    }
}

async fn handle_rx(rx: &mut mpsc::Receiver<Vec<String>>, arc_context: &RwLock<Context>) {
    while let Some(data) = rx.recv().await {
        let mut context = arc_context.write().await;

        for line in data {
            context.privmsg_all(&line);
        }
    }
}

async fn send<T: AsyncWrite>(
    writer: &mut WriteHalf<T>,
    arc_context: &RwLock<Context>,
) -> std::io::Result<()> {
    let mut len;
    {
        let context = arc_context.read().await;
        len = context.send_queue.len();
    }

    while len > 0 {
        let mut context = arc_context.write().await;
        let msg = context.send_queue.pop_front().unwrap();
        len -= 1;

        trace!(">> {}", msg.replace("\r\n", ""));
        let bytes_written = match writer.write(msg.as_bytes()).await {
            Ok(bytes_written) => bytes_written,
            Err(err) => match err.kind() {
                ErrorKind::WouldBlock => {
                    return Ok(());
                }
                _ => panic!("{err}"),
            },
        };

        if bytes_written < msg.len() {
            context
                .send_queue
                .push_front(msg[bytes_written..].to_owned());
        }
    }
    Ok(())
}

async fn recv<T: AsyncRead>(
    reader: &mut ReadHalf<T>,
    partial_line: &mut String,
) -> std::io::Result<Vec<String>> {
    let mut buf = [0; MAX_MSG_LEN];
    let mut lines = vec![];
    let bytes_read = match reader.read(&mut buf).await {
        Ok(bytes_read) => bytes_read,
        Err(err) => match err.kind() {
            ErrorKind::WouldBlock => {
                return Ok(lines);
            }
            _ => panic!("{err}"),
        },
    };

    if bytes_read == 0 {
        return Ok(lines);
    }

    let buf = &buf[..bytes_read];

    *partial_line += String::from_utf8_lossy(buf).into_owned().as_str();
    let new_lines: Vec<&str> = partial_line.split("\r\n").collect();
    let len = new_lines.len();

    for (index, line) in new_lines.into_iter().enumerate() {
        if index == len - 1 && &buf[buf.len() - 3..] != b"\r\n" {
            *partial_line = line.to_owned();
            break;
        }
        lines.push(line.to_owned());
    }
    Ok(lines)
}
