use irc::client::prelude::*;
use std::{fs};
use irc::client::prelude::{IrcClient, ClientExt, Client, Command};
use serde::Deserialize;
use regex::{RegexSet, Regex};
use std::process::Command as CliCommand;
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
use notify::{RecommendedWatcher, Watcher, RecursiveMode};
use std::sync::{Arc, Mutex};

const FRAGMENT: &AsciiSet = &CONTROLS.add(b' ').add(b'"').add(b'<').add(b'>').add(b'`');

#[derive(Deserialize, Clone, Debug)]
struct BotConfig {
    nickname: String,
    server: String,
    channels: Vec<String>,
    use_ssl: bool,
    torrent_patterns: Vec<String>,
    user_secret: String,
    on_new_torrent: String,
}

fn is_modify_data_event(event: &notify::Event) -> bool {
    match &event.kind {
        notify::EventKind::Modify(notify::event::ModifyKind::Data(_)) => true,
        _ => false
    }
}

fn main() {
    let bot_config = read_config().expect("Bad config :(");
    let bot_config = Arc::new(Mutex::new(bot_config));

    let bot_conf_mutex = bot_config.lock().unwrap();
    let patterns = RegexSet::new(&bot_conf_mutex.torrent_patterns[..]).expect("Couldn't create regex set");
    let match_set = Arc::new(Mutex::new(patterns));
    drop(bot_conf_mutex);

    let watcher_bot_config = bot_config.clone();
    let watcher_match_set = match_set.clone();
    let mut watcher: RecommendedWatcher = Watcher::new_immediate(move |res| {
        match res {
            Ok(event) => {
                if !is_modify_data_event(&event) {
                    return;
                };

                println!("Config file updated, reloading, {:?}", event);
                let mut conf_resource = watcher_bot_config.lock().unwrap();
                match read_config() {
                    Some(conf) => {
                        *conf_resource = conf;
                        println!("Conf updated");
                        match RegexSet::new(&conf_resource.torrent_patterns[..]) {
                            Ok(patterns) => {
                                let mut ms = watcher_match_set.lock().unwrap();
                                *ms = patterns;
                                drop(ms);
                                println!("Match set updated");
                            }
                            Err(e) => {
                                println!("Error, couldn't parse regex {}", e)
                            }
                        }
                    },
                    None => println!("Read config error")
                }
                //read_config().map(|conf| *conf_resource = conf);
            },
            Err(e) => println!("watch error: {:?}", e),
        }
    }).expect("Couldn't create watcher");

    watcher.watch("./botconfig.toml", RecursiveMode::NonRecursive).expect("Couldn't watch");




    let config = {

        let bot_config = bot_config.lock().unwrap();

        Config {
            nickname: Some(bot_config.nickname.to_string()),
            server: Some(bot_config.server.to_string()),
            channels: Some(bot_config.channels.clone()),
            use_ssl: Some(bot_config.use_ssl),
            ..Config::default()
        }
    };


    let client = IrcClient::from_config(config).unwrap();
    client.identify().unwrap();

    client.for_each_incoming(|irc_msg| {
        let secret_bot_config = &bot_config.lock().unwrap();
        let current_bot_config = secret_bot_config.clone();

        // irc_msg is a Message
        if let Command::PRIVMSG(_channel, message) = irc_msg.command {
            // use RegexSet

            let matches = match_set.lock().unwrap();

            println!("Received {}", message);
            match parse_torrent_announcement(message.clone()) {
                None => println!("Couldn't parse {}", message),
                Some((torrent_name, torrent_id)) => {
                    println!("Successfully parsed torrent announcment");
                    let torrent_name = utf8_percent_encode(&torrent_name, FRAGMENT);
                    let torrent_url = format!("https://www.torrentleech.org/rss/download/{}/{}/{}", torrent_id, current_bot_config.user_secret, torrent_name.to_string());
                    if matches.is_match(&message) {
                        //client.send_privmsg(&channel, "its a match").unwrap();
                        println!("Should download torrent and add to dl list {}", message);

                        let cli_command = str::replace(&current_bot_config.on_new_torrent, "$url", &torrent_url);
                        println!("Running command: {}", cli_command);
                        match CliCommand::new("sh").arg("-c").arg(cli_command).output() {
                            Err(e) => println!("Failed to execute, error: {}", e),
                            Ok(o) => {
                                if std::str::from_utf8(&o.stderr) != Ok("") {
                                    println!("Stderr when running command, {:?}", o)
                                } else {
                                    println!("Successfully ran command: {:?}", o)
                                }
                            }
                        }
                    } else {
                        println!("Doesn't match any patterns");
                    }



                },
            }


        }
    }).unwrap();
}

fn read_config() -> Option<BotConfig> {
    let bot_config = fs::read_to_string("botconfig.toml");//.ok()?;
    toml::from_str::<BotConfig>(&bot_config.unwrap()).ok()
}

fn parse_torrent_announcement(message: String) -> Option<(String, String)> {
    let pattern = r".*: <.+>  Name:'(.+)' uploaded by '(?:.+)' -  https://www.torrentleech.org/torrent/(\d+)";
    let r = Regex::new(pattern).unwrap();
    let groups = r.captures(&message)?;
    println!("{:?}", groups);

    if groups.len() != 3 { // match once for the whole string, once for the torrent name and once for the torrent id
        return None;
    }

    Some(
        (
            groups.get(1)?.as_str().to_owned(),
            groups.get(2)?.as_str().to_owned()
        )
    )

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_torrent_announcement() {
        let example_message = "New Torrent Announcement: <TV :: Episodes HD>  Name:'Antiques Roadshow US S24E03 Winterthur Museum Garden and Library Hour 3 720p WEB H264-GIMINI' uploaded by 'Anonymous' -  https://www.torrentleech.org/torrent/1558970".to_owned();
        assert_eq!(parse_torrent_announcement(example_message), Some(("Antiques Roadshow US S24E03 Winterthur Museum Garden and Library Hour 3 720p WEB H264-GIMINI".to_string(), "1558970".to_string())));
        let example_message = "New Torrent Announcement: <TV :: Episodes HD>  Name:'The Greatest Dancer S02E05 720p HDTV x264-QPEL' uploaded by 'Anonymous' -  https://www.torrentleech.org/torrent/1559045".to_owned();
        assert_eq!(parse_torrent_announcement(example_message), Some(("The Greatest Dancer S02E05 720p HDTV x264-QPEL".to_string(), "1559045".to_string())));
    }
}