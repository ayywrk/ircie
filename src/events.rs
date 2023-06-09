use log::{debug, info, warn};
use std::time::Duration;

use crate::{system::Response, Irc, IrcPrefix};

impl Irc {
    pub(crate) async fn event_ping(&mut self, ping_token: &str) {
        debug!("PING {}", ping_token);

        self.context
            .write()
            .await
            .queue(&format!("PONG {}", ping_token));
    }

    pub(crate) async fn event_welcome(&mut self, welcome_msg: &str) {
        debug!("{welcome_msg}");
        self.identify().await;

        let mut context = self.context.write().await;
        for channel in &self.config.channels {
            context.join(channel);
        }
    }

    pub(crate) async fn event_nicknameinuse(&mut self) {
        let mut context = self.context.write().await;
        let new_nick = format!("{}_", &self.config.nick);
        warn!("Nick already in use., switching to {}", new_nick);
        context.nick(&new_nick);
        self.config.nick = new_nick;
    }

    pub(crate) async fn event_kick(
        &mut self,
        channel: &str,
        nick: &str,
        kicker: &str,
        reason: &str,
    ) {
        let mut context = self.context.write().await;
        if nick != &self.config.nick {
            return;
        }

        warn!("We got kicked from {} by {}! ({})", channel, kicker, reason);
        context.join(channel);
    }

    pub(crate) async fn event_quit<'a>(&mut self, prefix: &'a IrcPrefix<'a>) {
        if prefix.nick != self.config.nick {
            return;
        }

        warn!("We quit. We'll reconnect in {} seconds.", 15);
        std::thread::sleep(Duration::from_secs(15));
        self.connect().await.unwrap();
    }

    pub(crate) async fn event_invite<'a>(&mut self, prefix: &'a IrcPrefix<'a>, channel: &str) {
        info!("{} invited us to {}", prefix.nick, channel);
        self.context.write().await.join(channel);
    }

    pub(crate) async fn event_notice<'a>(
        &mut self,
        _prefix: Option<&IrcPrefix<'a>>,
        channel: &str,
        message: &str,
    ) {
        let config = self.config.clone();

        if channel == &config.nick {
            if message.ends_with(&format!("\x02{}\x02 isn't registered.", config.nick)) {
                let nickserv_pass = config.nickserv_pass.as_ref().unwrap().to_string();
                let nickserv_email = config.nickserv_email.as_ref().unwrap().to_string();
                info!("Registering to nickserv now.");
                let mut context = self.context.write().await;
                context.privmsg(
                    "NickServ",
                    &format!("REGISTER {} {}", nickserv_pass, nickserv_email),
                );
            }
            if message.ends_with(" seconds to register.") {
                let seconds = message
                    .split_whitespace()
                    .nth(10)
                    .unwrap()
                    .parse::<usize>()
                    .unwrap()
                    + 1;

                info!("Waiting {} seconds to register.", seconds);
                /* TODO: fix this
                let ctx_clone = self.context.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(seconds as u64)).await;
                    self.identify().await;
                });
                 */
            }
        }
    }

    pub(crate) async fn event_privmsg<'a>(
        &mut self,
        prefix: &'a IrcPrefix<'a>,
        channel: &str,
        message: &str,
    ) {
        let mut elements;
        let sys_name;
        {
            let context = self.context.read().await;
            if !message.starts_with(&self.config.cmdkey) {
                return;
            }
            elements = message.split_whitespace();
            sys_name = elements.next().unwrap()[1..].to_owned();

            if prefix.owner() && sys_name == "raw" {
                drop(context);
                let mut context = self.context.write().await;
                context.queue(&elements.collect::<Vec<_>>().join(" "));
                return;
            }
        }

        if self.is_flood(channel).await {
            return;
        }

        let arguments = elements.collect::<Vec<_>>();

        if !self.systems.contains_key(&sys_name) {
            let resp = self.run_default_system(prefix, channel, &arguments).await;

            let Response::Data(data) = resp else {
                return;
            };

            let mut context = self.context.write().await;
            for (idx, line) in data.data.iter().enumerate() {
                if idx == 0 && data.highlight {
                    context.privmsg(channel, &format!("{}: {}", prefix.nick, line))
                } else {
                    context.privmsg(channel, &line)
                }
            }
            return;
        }

        let response = self.run_system(prefix, channel, &arguments, &sys_name).await;
        let Response::Data(data) = response else {
            return;
        };

        let mut context = self.context.write().await;
        for (idx, line) in data.data.iter().enumerate() {
            if idx == 0 && data.highlight {
                context.privmsg(channel, &format!("{}: {}", prefix.nick, line))
            } else {
                context.privmsg(channel, &line)
            }
        }
    }
}
