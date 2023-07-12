pub mod context;
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
use context::IrcContext;
use factory::Factory;
use irc_command::IrcCommand;
use log::{info, trace, warn};
use serde::{Deserialize, Serialize};
use system::{IntoSystem, Response, StoredSystem, System};
use tokio::{
    fs::File,
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadHalf, WriteHalf},
    net::TcpStream,
    sync::RwLock,
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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum IrcPrefixKind {
    Owner,
    Admin,
    #[default]
    User,
}

#[derive(Clone, Debug, Default)]
pub struct IrcPrefix<'a> {
    pub nick: &'a str,
    pub user: Option<&'a str>,
    pub host: Option<&'a str>,
    kind: IrcPrefixKind,
}

impl<'a> IrcPrefix<'a> {
    pub fn owner(&self) -> bool {
        self.kind == IrcPrefixKind::Owner
    }

    pub fn admin(&self) -> bool {
        self.kind == IrcPrefixKind::Admin || self.kind == IrcPrefixKind::Owner
    }
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
            ..Default::default()
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

#[derive(Clone, Serialize, Deserialize)]
pub struct IrcConfig {
    host: String,
    port: u16,
    ssl: bool,
    nick: String,
    channels: HashSet<String>,
    user: String,
    real: String,
    nickserv_pass: Option<String>,
    nickserv_email: Option<String>,
    cmdkey: String,
    flood_interval: f32,
    owner: String,
    admins: Vec<String>,
}

pub trait AsyncReadWrite: AsyncRead + AsyncWrite + Send + Unpin {}

impl<T: AsyncRead + AsyncWrite + Send + Unpin> AsyncReadWrite for T {}

pub struct Irc {
    context: Arc<RwLock<IrcContext>>,
    flood_controls: HashMap<String, FloodControl>,
    stream: Option<Box<dyn AsyncReadWrite>>,
    partial_line: String,

    config: IrcConfig,
    identified: bool,

    default_system: Option<StoredSystem>,
    invalid_system: Option<StoredSystem>,
    systems: HashMap<String, StoredSystem>,
    event_systems: HashMap<IrcCommand, StoredSystem>,
    tasks: Vec<(Duration, StoredSystem)>,
    factory: Arc<RwLock<Factory>>,
}

impl Irc {
    pub async fn from_config(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let mut file = File::open(path).await?;
        let mut contents = String::new();
        file.read_to_string(&mut contents).await?;

        let config: IrcConfig = serde_yaml::from_str(&contents).unwrap();

        let context = Arc::new(RwLock::new(IrcContext {
            send_queue: VecDeque::new(),
        }));

        Ok(Self {
            context,
            stream: None,
            flood_controls: HashMap::default(),
            partial_line: String::new(),
            config,
            identified: false,
            default_system: None,
            invalid_system: None,
            systems: HashMap::default(),
            event_systems: HashMap::default(),
            tasks: Vec::new(),
            factory: Arc::new(RwLock::new(Factory::default())),
        })
    }

    pub async fn add_system<I, S: for<'a> System + Send + Sync + 'static>(
        &mut self,
        name: &str,
        system: impl for<'a> IntoSystem<I, System = S>,
    ) -> &mut Self {
        self.systems
            .insert(name.to_owned(), Box::new(system.into_system()));
        self
    }

    pub async fn add_event_system<I, S: for<'a> System + Send + Sync + 'static>(
        &mut self,
        evt: IrcCommand,
        system: impl for<'a> IntoSystem<I, System = S>,
    ) -> &mut Self {
        self.event_systems
            .insert(evt, Box::new(system.into_system()));
        self
    }

    pub async fn add_default_system<I, S: for<'a> System + Send + Sync + 'static>(
        &mut self,
        system: impl for<'a> IntoSystem<I, System = S>,
    ) -> &mut Self {
        self.default_system = Some(Box::new(system.into_system()));
        self
    }

    pub async fn add_invalid_system<I, S: for<'a> System + Send + Sync + 'static>(
        &mut self,
        system: impl for<'a> IntoSystem<I, System = S>,
    ) -> &mut Self {
        self.invalid_system = Some(Box::new(system.into_system()));
        self
    }

    pub async fn add_interval_task<I, S: for<'a> System + Send + Sync + 'static>(
        &mut self,
        duration: Duration,
        system: impl for<'a> IntoSystem<I, System = S>,
    ) -> &mut Self {
        self.tasks.push((duration, Box::new(system.into_system())));
        self
    }

    pub async fn add_task<I, S: for<'a> System + Send + Sync + 'static>(
        &mut self,
        system: impl for<'a> IntoSystem<I, System = S>,
    ) -> &mut Self {
        self.tasks
            .push((Duration::ZERO, Box::new(system.into_system())));
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

    pub async fn connect(&mut self) -> std::io::Result<()> {
        let domain = format!("{}:{}", self.config.host, self.config.port);

        info!("Connecting to {}", domain);

        let mut addrs = domain
            .to_socket_addrs()
            .expect("Unable to get addrs from domain {domain}");

        let sock = addrs
            .next()
            .expect("Unable to get ip from addrs: {addrs:?}");

        let plain_stream = TcpStream::connect(sock).await?;

        if self.config.ssl {
            let stream = async_native_tls::connect(self.config.host.clone(), plain_stream)
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

        if elapsed.as_secs_f32() < self.config.flood_interval {
            warn!("they be floodin @ {channel}!");
            return true;
        }

        flood_control.last_cmd = SystemTime::now();
        false
    }

    fn is_owner(&self, prefix: &IrcPrefix) -> bool {
        let owner = ":".to_owned() + &self.config.owner;
        let owner_prefix: IrcPrefix = owner.as_str().into();

        if (owner_prefix.nick == prefix.nick || owner_prefix.nick == "*")
            && (owner_prefix.user == prefix.user || owner_prefix.user == Some("*"))
            && (owner_prefix.host == prefix.host || owner_prefix.host == Some("*"))
        {
            return true;
        }

        false
    }

    fn is_admin(&self, prefix: &IrcPrefix) -> bool {
        for admin_str in &self.config.admins {
            let admin = ":".to_owned() + admin_str;
            let admin_prefix: IrcPrefix = admin.as_str().into();

            if (admin_prefix.nick == prefix.nick || admin_prefix.nick == "*")
                && (admin_prefix.user == prefix.user || admin_prefix.user == Some("*"))
                && (admin_prefix.host == prefix.host || admin_prefix.host == Some("*"))
            {
                return true;
            }
        }

        false
    }

    pub fn into_message<'a>(&self, line: &'a str) -> IrcMessage<'a> {
        let mut message: IrcMessage = line.into();

        if let Some(prefix) = &mut message.prefix {
            if self.is_owner(prefix) {
                prefix.kind = IrcPrefixKind::Owner;
            } else if self.is_admin(prefix) {
                prefix.kind = IrcPrefixKind::Admin;
            }
        }

        message
    }

    pub async fn handle_commands(&mut self, mut lines: VecDeque<String>) {
        while lines.len() != 0 {
            let owned_line = lines.pop_front().unwrap();
            let line = owned_line.as_str();

            trace!("<< {:?}", line);

            let message = self.into_message(line);

            self.handle_message(&message).await;
        }
    }

    async fn handle_message<'a>(&mut self, message: &'a IrcMessage<'a>) {
        self.run_event_system(message).await;
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

    pub async fn register(&mut self) {
        info!(
            "Registering as {}!{} ({})",
            self.config.nick, self.config.user, self.config.real
        );
        let mut context = self.context.write().await;

        context.queue(&format!(
            "USER {} 0 * {}",
            self.config.user, self.config.real
        ));
        context.nick(&self.config.nick);
    }

    pub async fn identify(&mut self) {
        if self.config.nickserv_pass.is_none() || self.identified {
            return;
        }

        self.context.write().await.privmsg(
            "NickServ",
            &format!("IDENTIFY {}", self.config.nickserv_pass.as_ref().unwrap()),
        );
    }

    pub async fn run_system<'a>(
        &mut self,
        prefix: &'a IrcPrefix<'a>,
        channel: &'a str,
        arguments: &'a [&'a str],
        name: &str,
    ) -> Response {
        let system = self.systems.get_mut(name).unwrap();
        system.run(
            prefix,
            channel,
            arguments,
            &mut *self.context.write().await,
            &mut *self.factory.write().await,
        )
    }

    pub async fn run_event_system<'a>(&mut self, message: &'a IrcMessage<'a>) {
        let Some(system) = self.event_systems.get_mut(&message.command) else { return; };

        system.run(
            &message.prefix.clone().unwrap_or_default(),
            "",
            &message.parameters,
            &mut *self.context.write().await,
            &mut *self.factory.write().await,
        );
    }

    pub async fn run_default_system<'a>(
        &mut self,
        prefix: &'a IrcPrefix<'a>,
        channel: &'a str,
        arguments: &'a [&'a str],
    ) -> Response {
        if self.invalid_system.is_none() {
            return Response::Empty;
        }

        self.default_system.as_mut().unwrap().run(
            prefix,
            channel,
            arguments,
            &mut *self.context.write().await,
            &mut *self.factory.write().await,
        )
    }

    pub async fn run_invalid_system<'a>(
        &mut self,
        prefix: &'a IrcPrefix<'a>,
        channel: &'a str,
        arguments: &'a [&'a str],
    ) -> Response {
        if self.invalid_system.is_none() {
            return Response::Empty;
        }

        self.invalid_system.as_mut().unwrap().run(
            prefix,
            channel,
            arguments,
            &mut *self.context.write().await,
            &mut *self.factory.write().await,
        )
    }

    pub async fn run_interval_tasks(&mut self) {
        for (duration, mut task) in std::mem::take(&mut self.tasks) {
            let fact = self.factory.clone();
            let ctx = self.context.clone();
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(duration).await;
                    task.run(
                        &IrcPrefix::default(),
                        "",
                        &[],
                        &mut *ctx.write().await,
                        &mut *fact.write().await,
                    );
                }
            });
        }
    }

    pub async fn run(&mut self) -> std::io::Result<()> {
        self.connect().await?;
        info!("Ready!");
        self.register().await;

        self.run_interval_tasks().await;

        let stream = self.stream.take().unwrap();

        let (mut reader, mut writer) = tokio::io::split(stream);

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

async fn send<T: AsyncWrite>(
    writer: &mut WriteHalf<T>,
    arc_context: &RwLock<IrcContext>,
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

    let buf_str: String =
        partial_line.to_owned() + String::from_utf8_lossy(buf).into_owned().as_str();
    *partial_line = String::new();

    let new_lines: Vec<&str> = buf_str.split("\r\n").collect();
    let len = new_lines.len();

    for (index, line) in new_lines.into_iter().enumerate() {
        if buf.len() < 2 {
            println!("shit {:?}", buf);
            continue;
        }
        if index == len - 1 && &buf[buf.len() - 2..] != b"\r\n" {
            *partial_line = line.to_owned();
            break;
        }

        if line.len() != 0 {
            lines.push(line.to_owned());
        }
    }
    Ok(lines)
}
