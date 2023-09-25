pub mod rollup {
    use hyper::{body::to_bytes, header, Body, Client, Method, Request, StatusCode};
    use serde_json::{from_str, json, Value};
    use std::{env, error::Error, str::from_utf8, sync::Arc, time::Duration};
    use tokio::sync::{mpsc::Sender, Mutex};

    use crate::{
        models::{
            game::game::Manager,
            player::{check_fields_create_player, player::Player},
        },
        util::json::{
            decode_payload, generate_report, get_address_metadata_from_root, get_path_player,
            get_path_player_name, load_json, write_json,
        },
    };

    pub async fn rollup(
        manager: Arc<Mutex<Manager>>,
        sender: &Sender<Value>,
    ) -> Result<(), Box<dyn Error>> {
        println!("Starting loop...");

        let client = Client::new();
        // let https = HttpsConnector::new();
        // let client = Client::builder().build::<_, hyper::Body>(https);
        let server_addr = env::var("MIDDLEWARE_HTTP_SERVER_URL")?;

        let mut status = "accept";
        loop {
            println!("Sending finish");
            let response = json!({ "status": status.clone() });
            let request = Request::builder()
                .method(Method::POST)
                .header(header::CONTENT_TYPE, "application/json")
                .uri(format!("{}/finish", &server_addr))
                .body(Body::from(response.to_string()))?;
            let response = client.request(request).await?;
            let status_response = response.status();
            println!("Receive finish status {}", &status_response);

            if status_response == StatusCode::ACCEPTED {
                println!("No pending rollup request, trying again");
            } else {
                let body = to_bytes(response).await?;
                let body = from_utf8(&body)?;
                let body = from_str::<Value>(body)?;

                let request_type = body["request_type"]
                    .as_str()
                    .ok_or("request_type is not a string")?;

                status = match request_type {
                    "advance_state" => {
                        handle_advance(manager.clone(), &server_addr[..], body, sender).await?
                    }
                    "inspect_state" => {
                        handle_inspect(manager.clone(), &server_addr[..], body, sender).await?
                    }
                    &_ => {
                        eprintln!("Unknown request type");
                        "reject"
                    }
                }
            }
            println!("waiting 5s...");
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }

    async fn handle_inspect(
        manager: Arc<Mutex<Manager>>,
        server_addr: &str,
        body: Value,
        sender: &Sender<Value>,
    ) -> Result<&'static str, Box<dyn Error>> {
        println!("Handling inspect");

        println!("body {:}", &body);

        let result = handle_request_action(&body, manager, true).await?;

        if let Some(report) = result {
            send_report(report).await?;
        }

        Ok("accept")
    }

    async fn handle_advance(
        manager: Arc<Mutex<Manager>>,
        server_addr: &str,
        body: Value,
        sender: &Sender<Value>,
    ) -> Result<&'static str, Box<dyn Error>> {
        println!("Handling advance");

        // body {"data":{"metadata":{"block_number":321,"epoch_index":0,"input_index":0,"msg_sender":"0x70997970c51812dc3a010c7d01b50e0d17dc79c8","timestamp":1694789355},"payload":"0x7b22696e707574223a7b22616374696f6e223a226e65775f706c61796572222c226e616d65223a22416c696365227d7d"},"request_type":"advance_state"}
        println!("body {:}", &body);
        let run_async = std::env::var("RUN_GAME_ASYNC").unwrap_or("true".to_string());

        if run_async == "true" {
            sender.send(body).await?;
            return Ok("accept");
        }

        let result = handle_request_action(&body, manager, true).await?;
        if let Some(report) = result {
            send_report(report).await?;
        }

        Ok("accept")
    }

    pub(crate) async fn send_report(
        report: Value,
    ) -> Result<&'static str, Box<dyn std::error::Error>> {
        let server_addr = std::env::var("ROLLUP_HTTP_SERVER_URL")?;
        let client = hyper::Client::new();
        // let https = HttpsConnector::new();
        // let client = Client::builder().build::<_, hyper::Body>(https);
        let req = hyper::Request::builder()
            .method(hyper::Method::POST)
            .header(hyper::header::CONTENT_TYPE, "application/json")
            .uri(format!("{}/report", server_addr))
            .body(hyper::Body::from(report.to_string()))?;

        let _ = client.request(req).await?;
        Ok("accept")
    }

    pub fn get_payload_from_root(root: &Value) -> Option<Value> {
        let root = root.as_object()?;
        let root = root.get("data")?.as_object()?;
        let payload = root.get("payload")?.as_str()?;
        let payload = decode_payload(payload)?;
        Some(payload)
    }

    pub fn get_from_payload_action(payload: &Value) -> Option<String> {
        let input = payload.get("input")?.as_object()?;
        let action = input.get("action")?.as_str()?;
        Some(action.to_owned())
    }

    pub async fn handle_request_action(
        root: &Value,
        manager: Arc<Mutex<Manager>>,
        write_hd_mode: bool,
    ) -> Result<Option<Value>, &'static str> {
        let payload = get_payload_from_root(root).ok_or("Invalid payload")?;
        let action = get_from_payload_action(&payload);

        println!("Action: {:}", action.as_deref().unwrap_or("None"));

        match action.as_deref() {
            Some("new_player") => {
                let input = payload.get("input").ok_or("Invalid field input")?;
                let player_name = check_fields_create_player(&input)?;

                let encoded_name = bs58::encode(&player_name).into_string();

                let metadata = get_address_metadata_from_root(root).ok_or("Invalid address")?;
                let address_owner = metadata.address.trim_start_matches("0x");
                let address_encoded = bs58::encode(address_owner).into_string();

                // Add player to manager
                let player = Player::new(address_encoded.clone(), player_name.to_string());
                let mut manager = manager.lock().await;
                let player = Arc::new(player);
                manager.add_player(player)?;

                // Persist player
                if write_hd_mode {
                    let address_owner_obj =
                        json!({ "address": address_owner, "name": player_name });
                    let address_path = get_path_player(&address_encoded);

                    write_json(&address_path, &address_owner_obj)
                        .await
                        .or(Err("Could not write address"))?;

                    let player_path = get_path_player_name(&encoded_name);
                    let player = json!({ "name": encoded_name, "address": metadata.address });
                    write_json(&player_path, &player)
                        .await
                        .or(Err("Could not write player"))?;
                }

                let report = generate_report(json!({
                    "address": address_encoded,
                    "encoded_name": encoded_name,
                    "name": player_name,
                }));

                println!("Report: {:}", report);

                return Ok(Some(report));
            }
            Some("join_game") => {
                let input = payload.get("input").ok_or("Invalid field input")?;

                // Address
                let metadata = get_address_metadata_from_root(root).ok_or("Invalid address")?;
                let address_owner = metadata.address.trim_start_matches("0x");
                let address_encoded = bs58::encode(address_owner).into_string();

                // load to memory if not exists
                if write_hd_mode {
                    load_player_to_mem(&manager, &address_encoded).await?;
                }

                let mut manager = manager.lock().await;
                let player = manager.get_player_ref(&address_encoded)?;

                // Parsing JSON
                let game_id = input
                    .get("game_id")
                    .ok_or("Invalid field game_id")?
                    .as_str()
                    .ok_or("Invalid game_id")?;

                manager.player_join(game_id, player.clone())?;
                println!("Player joined: name {} game_id {}", player.name, game_id);
            }
            Some("show_player") => {
                let input = payload.get("input").ok_or("Invalid field input")?;

                // Parsing JSON
                let address = input
                    .get("address")
                    .ok_or("Invalid field address")?
                    .as_str()
                    .ok_or("Invalid address")?;
                let address_owner = address.trim_start_matches("0x");
                let address_encoded = bs58::encode(address_owner).into_string();

                // load to memory if not exists
                if write_hd_mode {
                    load_player_to_mem(&manager, &address_encoded).await?;
                }

                let manager = manager.lock().await;

                println!("lookin in table");
                let playing = manager
                    .tables
                    .iter()
                    .filter(|table| table.has_player(&address_encoded))
                    .map(|table| table.get_id())
                    .collect::<Vec<_>>();

                let player_borrow = manager.get_player_by_id(&address_encoded)?;

                let joined = manager
                    .games
                    .iter()
                    .filter(|game| game.has_player(&address_encoded))
                    .map(|game| game.get_id())
                    .collect::<Vec<_>>();

                let player = json!({
                    "name": player_borrow.name.clone(),
                    "address": address_owner,
                    "joined": joined,
                    "playing": playing,
                });
                let report = generate_report(player);

                return Ok(Some(report));
            }
            Some("show_games") => {
                let manager = manager.lock().await;
                let games = manager
                    .games
                    .iter()
                    .map(|game| {
                        json!({
                            "id": game.get_id(),
                            "players": game.players.len(),
                        })
                    })
                    .collect::<Vec<_>>();

                let report = generate_report(json!({
                    "games": games,
                }));

                println!("Report: {:}", report);

                return Ok(Some(report));
            }
            Some("start_game") => {
                let input = payload.get("input").ok_or("Invalid field input")?;
                let metadata = get_address_metadata_from_root(root).ok_or("Invalid address")?;
                let timestamp = metadata.timestamp;

                // Parsing JSON
                let game_id = input
                    .get("game_id")
                    .ok_or("Invalid field game_id")?
                    .as_str()
                    .ok_or("Invalid game_id")?;

                let mut manager = manager.lock().await;

                // Get game and make owner
                let game = manager.drop_game(game_id)?;

                let players: Vec<String> = game.players.iter().map(|p| p.get_id()).collect();

                // Generate table from game
                let mut table = game.round_start(2, metadata.timestamp)?;

                for _ in 0..2 {
                    for player_id in &players {
                        table.hit_player(player_id, timestamp).await?;
                    }
                }

                // Add table to manager
                manager.add_table(table);
                println!("Game started: game_id {}", game_id);
            }
            Some("stop_game") => {
                let input = payload.get("input").ok_or("Invalid field input")?;

                // Parsing JSON
                let game_id = input
                    .get("game_id")
                    .ok_or("Invalid field game_id")?
                    .as_str()
                    .ok_or("Invalid game_id")?;

                let mut manager = manager.lock().await;

                manager.stop_game(game_id).await?;
            }
            Some("show_hands") => {
                let input = payload.get("input").ok_or("Invalid field input")?;

                // Parsing JSON
                let game_id = input
                    .get("game_id")
                    .ok_or("Invalid field game_id")?
                    .as_str()
                    .ok_or("Invalid game_id")?;

                let mut manager = manager.lock().await;

                let table = manager.get_table(game_id)?;
                let hands = table.generate_hands();
                let report = generate_report(hands);

                println!("Report: {:}", report);

                return Ok(Some(report));
            }
            Some("show_winner") => {
                let input = payload.get("input").ok_or("Invalid field input")?;

                // Parsing JSON
                let game_id = input
                    .get("game_id")
                    .ok_or("Invalid field game_id")?
                    .as_str()
                    .ok_or("Invalid game_id")?;

                let table_id = input
                    .get("table_id")
                    .ok_or("Invalid field table_id")?
                    .as_str()
                    .ok_or("Invalid string table_id")?;

                let manager = manager.lock().await;

                println!(
                    "Finding score by table_id {} and game_id {} ...",
                    table_id, game_id
                );
                let scoreboard = manager
                    .get_scoreboard(table_id, game_id)
                    .ok_or("Scoreboard not found searching by table_id")?;

                let report = generate_report(scoreboard.to_json());

                println!("Report: {:}", report);

                return Ok(Some(report));
            }
            Some("hit") => {
                // Address
                let metadata = get_address_metadata_from_root(root).ok_or("Invalid address")?;
                let address_owner = metadata.address.trim_start_matches("0x");
                let address_encoded = bs58::encode(address_owner).into_string();
                let timestamp = metadata.timestamp;

                // Game ID
                let input = payload.get("input").ok_or("Invalid field input")?;
                let game_id = input
                    .get("game_id")
                    .ok_or("Invalid field game_id")?
                    .as_str()
                    .ok_or("Invalid game_id")?;

                let mut manager = manager.lock().await;
                let table = manager.get_table(game_id)?;
                let table_id = table.get_id().to_owned();
                table.hit_player(&address_encoded, timestamp).await?;

                if !table.any_player_can_hit() {
                    manager.stop_game(&table_id).await?;
                }
            }
            Some("stand") => {
                let input = payload.get("input").ok_or("Invalid field input")?;

                // Parsing JSON
                let game_id = input
                    .get("game_id")
                    .ok_or("Invalid field game_id")?
                    .as_str()
                    .ok_or("Invalid game_id")?;

                let metadata = get_address_metadata_from_root(root).ok_or("Invalid address")?;
                let address_owner = metadata.address.trim_start_matches("0x");
                let address_encoded = bs58::encode(address_owner).into_string();

                let mut manager = manager.lock().await;
                let table = manager.get_table(game_id)?;

                let name = table.get_name_player(&address_encoded).unwrap();
                let table_id = table.get_id().to_owned();
                table.stand_player(&address_encoded, metadata.timestamp)?;

                if !table.any_player_can_hit() {
                    manager.stop_game(&table_id).await?;
                }
                println!("Stand: {} game_id {}", name, game_id);
            }
            _ => Err("Invalid action")?,
        }

        Ok(None)
    }

    async fn load_player_to_mem(
        manager: &Arc<Mutex<Manager>>,
        address_encoded: &String,
    ) -> Result<(), &'static str> {
        let mut manager = manager.lock().await;
        let has_player_in_memory = manager.has_player(address_encoded);
        if !has_player_in_memory {
            let path = get_path_player(address_encoded);
            let player = load_json(&path)
                .await
                .map_err(|_| "Could not load player")?;

            let player = player.as_object().ok_or("Invalid player")?;
            let player_name = player.get("name").ok_or("Invalid field name")?;
            let player_name = player_name.as_str().ok_or("Invalid name")?;

            let player = Player::new(address_encoded.clone(), player_name.to_string());
            let player = Arc::new(player);
            manager.add_player(player)?;
        }
        Ok(())
    }
}
