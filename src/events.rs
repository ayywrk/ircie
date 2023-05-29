use std::time::Duration;

use crate::{Irc, IrcPrefix};

impl Irc {
    pub(crate) fn event_ping(&mut self, ping_token: &str) {
        self.queue(&format!("PONG {}", ping_token));
    }

    pub(crate) fn event_welcome(&mut self) {
        // self.identify();
        self.join_config_channels();
    }

    pub(crate) fn event_nicknameinuse(&mut self) {
        self.update_nick(&format!("{}_", &self.config.nick))
    }

    pub(crate) fn event_kick(&mut self, channel: &str, nick: &str, message: &str) {
        if nick != &self.config.nick {
            return;
        }

        println!("we got kicked!");
        println!("{message}");

        self.join(channel);
    }

    pub(crate) async fn event_quit<'a>(&mut self, prefix: &'a IrcPrefix<'a>) {
        if prefix.nick != self.config.nick {
            return;
        }

        println!("need to reconnect.");
        std::thread::sleep(Duration::from_secs(15));
        self.connect().await.unwrap();
        self.register();
    }

    pub(crate) fn event_invite(&mut self, prefix: &IrcPrefix, channel: &str) {
        println!("{} invited us to {}", prefix.nick, channel);
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
        self.run_system(prefix, sys_name);
    }
}
