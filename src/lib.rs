pub mod events;
pub mod factory;
pub mod irc_command;
pub mod system;
pub mod system_params;

use std::{
    any::TypeId,
    collections::{HashMap, VecDeque},
    io::ErrorKind,
    net::ToSocketAddrs,
    path::Path,
    time::SystemTime,
};

use async_native_tls::TlsStream;
use factory::Factory;
use irc_command::IrcCommand;
use serde::{Deserialize, Serialize};
use system::{IntoSystem, StoredSystem, System};
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
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
    pub admin: bool,
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
            admin: false,
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
    channels: Vec<String>,
    nick: String,
    user: String,
    real: String,
    nickserv_pass: String,
    nickserv_email: String,
    cmdkey: String,
    flood_interval: f32,
    owner: String,
    admins: Vec<String>,
}

pub struct Irc {
    config: IrcConfig,
    stream: Stream,

    systems: HashMap<String, StoredSystem>,
    factory: Factory,

    flood_controls: HashMap<String, FloodControl>,

    send_queue: VecDeque<String>,
    recv_queue: VecDeque<String>,
    partial_line: String,
}

impl Irc {
    pub async fn from_config(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let mut file = File::open(path).await?;
        let mut contents = String::new();
        file.read_to_string(&mut contents).await?;

        let config: IrcConfig = serde_yaml::from_str(&contents).unwrap();

        Ok(Self {
            config,
            stream: Stream::None,
            systems: HashMap::default(),
            factory: Factory::default(),
            flood_controls: HashMap::default(),
            send_queue: VecDeque::new(),
            recv_queue: VecDeque::new(),
            partial_line: String::new(),
        })
    }

    pub fn add_system<I, S: for<'a> System<'a> + 'static>(
        &mut self,
        name: &str,
        system: impl for<'a> IntoSystem<'a, I, System = S>,
    ) -> &mut Self {
        self.systems
            .insert(name.to_owned(), Box::new(system.into_system()));
        self
    }

    pub fn add_resource<R: 'static>(&mut self, res: R) -> &mut Self {
        self.factory
            .resources
            .insert(TypeId::of::<R>(), Box::new(res));
        self
    }

    pub fn run_system<'a>(&mut self, prefix: &'a IrcPrefix, name: &str) {
        let system = self.systems.get_mut(name).unwrap();
        system.run(prefix, &mut self.factory);
    }

    pub async fn connect(&mut self) -> std::io::Result<()> {
        let domain = format!("{}:{}", self.config.host, self.config.port);

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
            self.stream = Stream::Tls(stream);
            return Ok(());
        }

        self.stream = Stream::Plain(plain_stream);
        Ok(())
    }

    pub fn register(&mut self) {
        self.queue(&format!(
            "USER {} 0 * {}",
            self.config.user, self.config.real
        ));
        self.queue(&format!("NICK {}", self.config.nick));
    }

    async fn recv(&mut self) -> std::io::Result<()> {
        let mut buf = [0; MAX_MSG_LEN];

        let bytes_read = match self.stream.read(&mut buf).await {
            Ok(bytes_read) => bytes_read,
            Err(err) => match err.kind() {
                ErrorKind::WouldBlock => {
                    return Ok(());
                }
                _ => panic!("{err}"),
            },
        };

        if bytes_read == 0 {
            return Ok(());
        }

        let buf = &buf[..bytes_read];

        self.partial_line += String::from_utf8_lossy(buf).into_owned().as_str();
        let new_lines: Vec<&str> = self.partial_line.split("\r\n").collect();
        let len = new_lines.len();

        for (index, line) in new_lines.into_iter().enumerate() {
            if index == len - 1 && &buf[buf.len() - 3..] != b"\r\n" {
                self.partial_line = line.to_owned();
                break;
            }
            self.recv_queue.push_back(line.to_owned());
        }
        Ok(())
    }

    async fn send(&mut self) -> std::io::Result<()> {
        while self.send_queue.len() > 0 {
            let msg = self.send_queue.pop_front().unwrap();

            let bytes_written = match self.stream.write(msg.as_bytes()).await {
                Ok(bytes_written) => bytes_written,
                Err(err) => match err.kind() {
                    ErrorKind::WouldBlock => {
                        return Ok(());
                    }
                    _ => panic!("{err}"),
                },
            };

            if bytes_written < msg.len() {
                self.send_queue.push_front(msg[bytes_written..].to_owned());
            }
        }
        Ok(())
    }

    fn queue(&mut self, msg: &str) {
        let mut msg = msg.replace("\r", "").replace("\n", "");

        if msg.len() > MAX_MSG_LEN - "\r\n".len() {
            let mut i = 0;

            while i < msg.len() {
                let max = (MAX_MSG_LEN - "\r\n".len()).min(msg[i..].len());

                let mut m = msg[i..(i + max)].to_owned();
                println!(">> {:?}", m);
                m = m + "\r\n";
                self.send_queue.push_back(m);
                i += MAX_MSG_LEN - "\r\n".len()
            }
        } else {
            println!(">> {:?}", msg);
            msg = msg + "\r\n";
            self.send_queue.push_back(msg);
        }
    }

    pub async fn update(&mut self) -> std::io::Result<()> {
        self.recv().await?;
        self.send().await?;
        self.handle_commands().await;
        Ok(())
    }

    pub async fn handle_commands(&mut self) {
        while self.recv_queue.len() != 0 {
            let owned_line = self.recv_queue.pop_front().unwrap();
            let line = owned_line.as_str();

            println!("<< {:?}", line);

            let mut message: IrcMessage = line.into();

            let Some(prefix) = &mut message.prefix else {
                return self.handle_message(&message).await;
            };

            if self.is_owner(prefix) {
                prefix.admin = true;
            } else {
                for admin in &self.config.admins {
                    if self.is_admin(prefix, admin) {
                        prefix.admin = true;
                        break;
                    }
                }
            }

            self.handle_message(&message).await;
        }
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
        self.queue(&format!("JOIN {}", channel))
    }

    fn join_config_channels(&mut self) {
        for i in 0..self.config.channels.len() {
            let channel = &self.config.channels[i];
            self.queue(&format!("JOIN {}", channel))
        }
    }

    fn update_nick(&mut self, new_nick: &str) {
        self.config.nick = new_nick.to_owned();
        self.queue(&format!("NICK {}", self.config.nick));
    }

    fn is_flood(&mut self, channel: &str) -> bool {
        let mut flood_control = match self.flood_controls.entry(channel.to_owned()) {
            std::collections::hash_map::Entry::Occupied(o) => o.into_mut(),
            std::collections::hash_map::Entry::Vacant(v) => v.insert(FloodControl {
                last_cmd: SystemTime::now(),
            }),
        };

        let elapsed = flood_control.last_cmd.elapsed().unwrap();

        if elapsed.as_secs_f32() < self.config.flood_interval {
            return true;
        }

        flood_control.last_cmd = SystemTime::now();
        false
    }

    pub fn privmsg(&mut self, channel: &str, message: &str) {
        self.queue(&format!("PRIVMSG {} :{}", channel, message));
    }

    pub fn privmsg_all(&mut self, message: &str) {
        for i in 0..self.config.channels.len() {
            let channel = &self.config.channels[i];
            self.queue(&format!("PRIVMSG {} :{}", channel, message));
        }
    }

    async fn handle_message<'a>(&mut self, message: &'a IrcMessage<'a>) {
        match message.command {
            IrcCommand::PING => self.event_ping(&message.parameters[0]),
            IrcCommand::RPL_WELCOME => self.event_welcome(),
            IrcCommand::ERR_NICKNAMEINUSE => self.event_nicknameinuse(),
            IrcCommand::KICK => self.event_kick(
                message.parameters[0],
                message.parameters[1],
                &message.parameters[3..].join(" "),
            ),
            IrcCommand::QUIT => self.event_quit(message.prefix.as_ref().unwrap()).await,
            IrcCommand::INVITE => self.event_invite(
                message.prefix.as_ref().unwrap(),
                &message.parameters[0][1..],
            ),
            IrcCommand::PRIVMSG => self.event_privmsg(
                message.prefix.as_ref().unwrap(),
                &message.parameters[0],
                &message.parameters[1..].join(" ")[1..],
            ),
            IrcCommand::NOTICE => self.event_notice(
                message.prefix.as_ref(),
                &message.parameters[0],
                &message.parameters[1..].join(" ")[1..],
            ),
            _ => {}
        }
    }
}
