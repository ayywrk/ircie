use std::time::Duration;

use log::{debug, info, warn};

use crate::{Irc, IrcPrefix};

impl Irc {
    pub(crate) fn event_ping(&mut self, ping_token: &str) {
        debug!("PING {}", ping_token);
        self.queue(&format!("PONG {}", ping_token));
    }

    pub(crate) fn event_welcome(&mut self, welcome_msg: &str) {
        debug!("{welcome_msg}");
        // self.identify();
        self.join_config_channels();
    }

    pub(crate) fn event_nicknameinuse(&mut self) {
        let new_nick = &format!("{}_", &self.config.nick);
        warn!("Nick already in use., switching to {}", new_nick);
        self.update_nick(new_nick)
    }

    pub(crate) fn event_kick(&mut self, channel: &str, nick: &str, kicker: &str, reason: &str) {
        if nick != &self.config.nick {
            return;
        }

        warn!("We got kicked from {} by {}! ({})", channel, kicker, reason);
        self.join(channel);
    }

    pub(crate) async fn event_quit<'a>(&mut self, prefix: &'a IrcPrefix<'a>) {
        if prefix.nick != self.config.nick {
            return;
        }

        warn!("We quit. We'll reconnect in {} seconds.", 15);
        std::thread::sleep(Duration::from_secs(15));
        self.connect().await.unwrap();
        self.register();
    }

    pub(crate) fn event_invite(&mut self, prefix: &IrcPrefix, channel: &str) {
        info!("{} invited us to {}", prefix.nick, channel);
        self.join(channel);
    }

    pub(crate) fn event_notice(
        &mut self,
        prefix: Option<&IrcPrefix>,
        channel: &str,
        message: &str,
    ) {
        //TODO, register shit
    }

    pub(crate) fn event_privmsg(&mut self, prefix: &IrcPrefix, channel: &str, message: &str) {
        if !message.starts_with(&self.config.cmdkey) {
            return;
        }
        let mut elements = message.split_whitespace();
        let sys_name = &elements.next().unwrap()[1..];

        if self.is_owner(prefix) && sys_name == "raw" {
            self.queue(&elements.collect::<Vec<_>>().join(" "));
            return;
        }

        if self.is_flood(channel) {
            return;
        }

        let response = self.run_system(prefix, sys_name);

        if response.0.is_none() {
            return;
        }

        for line in response.0.unwrap() {
            self.privmsg(channel, &line)
        }
    }
}
