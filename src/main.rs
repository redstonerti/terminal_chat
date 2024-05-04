use addr::parse_domain_name;
use crossterm::{
    execute,
    terminal::{Clear, ClearType},
};
use dialoguer::{theme::ColorfulTheme, Input, Select};
use std::{
    io::{stdout, BufReader, ErrorKind, Read, Write},
    net::{Ipv4Addr, TcpListener, TcpStream, ToSocketAddrs},
    sync::{
        mpsc::{channel, Receiver, Sender},
        Arc, Mutex,
    },
    thread,
};
use terminal_chat::ThreadPool;
const PORT: &str = "10212";
#[derive(Debug)]
struct Player {
    id: u32,
    name: String,
    stream: TcpStream,
}
enum PlayerAction {
    Joined,
    SentMessage(String),
    Exited,
}
fn main() {
    main_menu();
}
fn _clear() {
    execute!(stdout(), Clear(ClearType::All)).unwrap();
    print!("\x1B[2J\x1B[1;1H");
    execute!(stdout(), Clear(ClearType::All)).unwrap();
    print!("\x1B[2J\x1B[1;1H");
}
fn clear_line() {
    print!("\x1B[2K");
    print!("\r");
    std::io::stdout().flush().unwrap();
}
fn main_menu() {
    let options = vec!["Host", "Connect to Host"];
    let selection = Select::with_theme(&ColorfulTheme::default())
        .items(&options)
        .interact()
        .unwrap();

    match selection {
        0 => {
            match TcpListener::bind(&format!("0.0.0.0:{PORT}")) {
                Ok(listener) => {
                    host(listener);
                }
                Err(err) => {
                    println!("Failed to create TcpListener: {err:#?}");
                    main_menu();
                    std::process::exit(0);
                }
            };
        }
        1 => {
            let ip: String = Input::with_theme(&ColorfulTheme::default())
                .with_prompt("Enter IP address or domain name")
                .validate_with(|x: &String| {
                    match x.parse::<Ipv4Addr>() {
                        Ok(_) => return Ok(()),
                        Err(_) => match parse_domain_name(x) {
                            Ok(name) => {
                                if name.to_string().contains(".") {
                                    return Ok(());
                                } else {
                                    return Err("Invalid domain name".to_owned());
                                }
                            }
                            Err(_) => {
                                return Err("Couldn't parse domain name or ip address".to_owned())
                            }
                        },
                    };
                })
                .interact()
                .unwrap();
            let ip = format!("{ip}:{PORT}")
                .to_socket_addrs()
                .unwrap()
                .next()
                .unwrap();
            match TcpStream::connect(ip) {
                Ok(stream) => {
                    client(stream);
                }
                Err(err) => {
                    println!("Failed to connect to tcpstream: {err:#?}");
                    main_menu();
                    std::process::exit(0);
                }
            };
        }
        _ => {}
    };
}
fn host(listener: TcpListener) {
    let players: Vec<Player> = vec![];
    let players_mutex = Arc::new(Mutex::new(players));
    let pool = ThreadPool::new(10);
    let players_mutex1 = Arc::clone(&players_mutex);
    let mut count = 0;
    let (tx, rx): (
        Sender<(u32, Option<String>, PlayerAction)>,
        Receiver<(u32, Option<String>, PlayerAction)>,
    ) = channel();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let stream = stream.unwrap();
            let players_mutex2 = Arc::clone(&players_mutex1);
            let tx1 = tx.clone();
            pool.execute(move || {
                handle_connection(stream, Arc::clone(&players_mutex2), count, tx1.clone());
            });
            count += 1;
        }
    });
    while let Ok(received) = rx.recv() {
        if let Some(sender_name) = received.1 {
            match received.2 {
                PlayerAction::Joined => {
                    send_to_players(
                        received.0,
                        format!("{} has joined the server!", sender_name),
                        Arc::clone(&players_mutex),
                    );
                    let player_amount = players_mutex.lock().unwrap().len();
                    let join_message: String;
                    if player_amount > 1 {
                        join_message = format!("Joined server with {player_amount} people!");
                    } else {
                        join_message = format!("Oh no! It looks like you're the only one here... Don't worry, i'm sure more people will join soon!");
                    }
                    for player in players_mutex.lock().unwrap().iter_mut() {
                        if player.id == received.0 {
                            player
                                .stream
                                .write_all(format!("{join_message}\\t").as_bytes())
                                .unwrap();
                            break;
                        }
                    }
                }
                PlayerAction::SentMessage(message) => {
                    send_to_players(
                        received.0,
                        format!("{}: {}", sender_name, message),
                        Arc::clone(&players_mutex),
                    );
                }
                PlayerAction::Exited => {
                    send_to_players(
                        received.0,
                        format!("{} has left the server :(", sender_name),
                        Arc::clone(&players_mutex),
                    );
                }
            }
        }
    }
}
fn send_to_players(id: u32, message: String, players: Arc<Mutex<Vec<Player>>>) {
    for player in players.lock().unwrap().iter_mut() {
        if id != player.id {
            player
                .stream
                .write_all(format!("{}\\t", message).as_bytes())
                .unwrap();
        }
    }
}
fn handle_connection(
    mut stream: TcpStream,
    players: Arc<Mutex<Vec<Player>>>,
    id: u32,
    tx: Sender<(u32, Option<String>, PlayerAction)>,
) {
    let mut first_time = true;
    let mut name: Option<String> = None;
    send_players(&mut stream, Arc::clone(&players), id);
    loop {
        match get_messages_from_stream(&mut stream) {
            Ok(messages) => {
                for message in messages {
                    if first_time {
                        name = Some(message.clone());
                        let player = Player {
                            id,
                            name: message.clone(),
                            stream: stream.try_clone().unwrap(),
                        };
                        players.lock().unwrap().push(player);
                        first_time = false;
                        tx.send((id, name.clone(), PlayerAction::Joined)).unwrap();
                    } else {
                        tx.send((id, name.clone(), PlayerAction::SentMessage(message)))
                            .unwrap();
                    }
                }
            }
            Err(_) => {
                let mut players = players.lock().unwrap();
                for i in 0..players.len() {
                    if players[i].id == id {
                        players.remove(i);
                        break;
                    }
                }
                tx.send((id, name.clone(), PlayerAction::Exited)).unwrap();
                break;
            }
        }
    }
}
fn send_players(stream: &mut TcpStream, players: Arc<Mutex<Vec<Player>>>, id: u32) {
    let mut player_names = String::from("");
    for player in players.lock().unwrap().iter() {
        if player.id != id {
            player_names = format!("{player_names} {}", player.name);
        }
    }
    player_names = format!("{player_names}\\t");
    stream.write_all(player_names.as_bytes()).unwrap();
}
fn client(mut stream: TcpStream) {
    let mut player_names: Option<String> = None;
    loop {
        let names = match get_messages_from_stream(&mut stream) {
            Ok(names) => {
                if names.len() > 0 {
                    names[0].clone()
                } else {
                    continue;
                }
            }
            Err(err) => {
                panic!("{err:#?}");
            }
        };
        if names.len() > 0 {
            player_names = Some(names);
        }
        break;
    }
    let name: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Enter name: ")
        .validate_with(|x: &String| {
            if let Some(player_names) = player_names.clone() {
                if player_names.contains(x) {
                    return Err("Name is already taken");
                }
            }
            if x.contains("#") {
                Err("Name cannot contain hashtags")
            } else {
                Ok(())
            }
        })
        .interact()
        .unwrap();
    if let Err(err) = stream.write_all(format!("{name}\\t").as_bytes()) {
        handle_client_errors(err);
    }
    let mut stream_copy = stream.try_clone().unwrap();
    let name_copy = name.clone();
    thread::spawn(move || loop {
        match get_messages_from_stream(&mut stream_copy) {
            Ok(messages) => {
                for message in messages {
                    clear_line();
                    print!("{message}\n{name_copy}: ");
                    std::io::stdout().flush().unwrap();
                }
            }
            Err(err) => handle_client_errors(err),
        }
    });
    loop {
        let mut str = String::from("");
        print!("{name}: ");
        std::io::stdout().flush().unwrap();
        std::io::stdin().read_line(&mut str).unwrap();
        if str.contains("\\t") {
            println!("Cannot send string that contains \\t");
            continue;
        }
        str = str.trim().to_string();
        str = format!("{str}\\t");
        stream.write_all(str.as_bytes()).unwrap();
    }
}
fn read_stream(mut stream: &mut TcpStream) -> Result<String, std::io::Error> {
    let mut reader = BufReader::new(&mut stream);
    let mut str = vec![];
    for _ in 0..512 {
        str.push(0u8);
    }
    match reader.read(&mut str) {
        Err(err) => {
            return Err(err);
        }
        _ => {}
    }
    for i in 0..512 {
        let i = ((512 - i as i32) - 1).max(0) as usize;
        if str[i] == 0 {
            str.pop();
        } else {
            break;
        }
    }
    return Ok(String::from_utf8(str.to_vec()).unwrap());
}
fn get_messages_from_stream(stream: &mut TcpStream) -> Result<Vec<String>, std::io::Error> {
    let mut messages = vec![];
    let mut total_stream = String::new();
    let str = match read_stream(stream) {
        Ok(data) => data,
        Err(err) => return Err(err),
    };
    total_stream = format!("{total_stream}{str}");
    loop {
        if let None = total_stream.find("\\t") {
            break;
        }
        let new_stream = total_stream.clone();
        let mut messages_in_stream = new_stream.split("\\t");
        if messages_in_stream.clone().count() > 1 {
            messages.push(messages_in_stream.next().unwrap().to_string());
            total_stream = String::from("");
            while let Some(new_message) = messages_in_stream.next() {
                total_stream = format!("{total_stream}{new_message}");
            }
        }
    }
    Ok(messages)
}
fn handle_client_errors(err: std::io::Error) {
    clear_line();
    match err.kind() {
        ErrorKind::ConnectionReset => {
            println!("Server closed");
        }
        _ => {
            println!("Exiting because: {err:?}");
        }
    }
    std::process::exit(0);
}
