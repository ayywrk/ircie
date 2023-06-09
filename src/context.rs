use std::collections::VecDeque;

use log::{debug, info};

use crate::MAX_MSG_LEN;

pub struct IrcContext {
    pub(crate) send_queue: VecDeque<String>,
}

impl IrcContext {
    pub fn privmsg(&mut self, channel: &str, message: &str) {
        debug!("sending privmsg to {} : {}", channel, message);
        self.queue(&format!("PRIVMSG {} :{}", channel, message));
    }
    pub(crate) fn queue(&mut self, msg: &str) {
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

    pub fn join(&mut self, channel: &str) {
        info!("Joining {channel}");
        self.queue(&format!("JOIN {}", channel));
    }

    pub fn nick(&mut self, nick: &str) {
        self.queue(&format!("NICK {}", nick));
    }

    pub fn mode(&mut self, channel: &str, modes: &str) {
        self.queue(&format!("MODE {} {}", channel, modes))
    }
}
