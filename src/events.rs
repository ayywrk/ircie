use log::{debug, info, warn};
use std::time::Duration;

use crate::{Irc, IrcPrefix};

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
        let mut context = self.context.write().await;
        context.identify();
        context.join_config_channels();
    }

    pub(crate) async fn event_nicknameinuse(&mut self) {
        let mut context = self.context.write().await;
        let new_nick = &format!("{}_", &context.config.nick);
        warn!("Nick already in use., switching to {}", new_nick);
        context.update_nick(new_nick)
    }

    pub(crate) async fn event_kick(
        &mut self,
        channel: &str,
        nick: &str,
        kicker: &str,
        reason: &str,
    ) {
        let mut context = self.context.write().await;
        if nick != &context.config.nick {
            return;
        }

        warn!("We got kicked from {} by {}! ({})", channel, kicker, reason);
        context.join(channel);
    }

    pub(crate) async fn event_quit<'a>(&mut self, prefix: &'a IrcPrefix<'a>) {
        if prefix.nick != self.context.read().await.config.nick {
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
        let config = self.context.read().await.config.clone();

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
                let ctx_clone = self.context.clone();

                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(seconds as u64)).await;
                    ctx_clone.write().await.identify();
                });
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
            if !message.starts_with(&context.config.cmdkey) {
                return;
            }
            elements = message.split_whitespace();
            sys_name = elements.next().unwrap()[1..].to_owned();

            if context.is_owner(prefix) && sys_name == "raw" {
                let mut context = self.context.write().await;
                context.queue(&elements.collect::<Vec<_>>().join(" "));
                return;
            }
        }

        if self.is_flood(channel).await {
            return;
        }

        let mut context = self.context.write().await;
        if !context.systems.contains_key(&sys_name) {
            return;
        }
        let response = context
            .run_system(prefix, elements.collect(), &sys_name)
            .await;

        match response {
            crate::system::Response::Lines(lines) => {
                for line in lines {
                    context.privmsg(channel, &line)
                }
            }
            crate::system::Response::Empty => return,
            crate::system::Response::InvalidArgument => return,
        }
    }
}
