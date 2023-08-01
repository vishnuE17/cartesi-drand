pub mod game {
    use crate::{
        models::{
            card::card::Deck,
            player::player::{Credit, Player, PlayerHand},
        },
        util::random::generate_id,
    };
    use std::{borrow::BorrowMut, sync::Arc};
    use tokio::sync::Mutex;

    pub struct Manager {
        pub games: Vec<Game>,
        pub players: Vec<Player>,
    }

    impl Default for Manager {
        fn default() -> Self {
            let games = Vec::new();

            Manager {
                games,
                players: Vec::new(),
            }
        }
    }

    impl Manager {
        pub fn new_with_games(game_size: usize) -> Self {
            let mut games = Vec::with_capacity(game_size);

            for _ in 0..game_size {
                games.push(Game::default());
            }

            Manager {
                games,
                players: Vec::new(),
            }
        }

        pub fn add_player(&mut self, player: Player) -> Result<(), &'static str> {
            self.players.push(player);
            Ok(())
        }

        pub fn remove_player_by_id(&mut self, id: String) -> Result<Player, &'static str> {
            let index = self
                .players
                .iter()
                .position(|player| player.get_id() == id)
                .ok_or("Player not found.")?;
            let player = self.players.remove(index);
            Ok(player)
        }

        pub fn first_game_available(&mut self) -> Result<&mut Game, &'static str> {
            self.games.first_mut().ok_or("No games available.")
        }

        pub fn show_games_id_available(&self) -> Vec<String> {
            self.games.iter().map(|game| game.id.clone()).collect()
        }

        pub fn drop_game(&mut self, id: String) -> Result<Game, &'static str> {
            let index = self
                .games
                .iter()
                .position(|game| game.id == id)
                .ok_or("Game not found.")?;
            let game = self.games.remove(index);
            Ok(game)
        }

        pub fn realocate_table_to_game(&mut self, table: Table) {
            self.games.push(table.game);
        }
    }

    /**
     * This is where the game is initialized.
     */
    pub struct Game {
        id: String,
        pub players: Vec<Arc<Mutex<Player>>>,
    }

    impl Default for Game {
        fn default() -> Self {
            Game {
                id: generate_id(),
                players: Vec::new(),
            }
        }
    }

    impl Game {
        pub fn get_id(&self) -> &str {
            &self.id
        }

        pub fn player_join(&mut self, player: Player) -> Result<(), &'static str> {
            if self.players.len() >= 7 {
                return Err("Maximum number of players reached.");
            }

            let player = Arc::new(Mutex::new(player));

            self.players.push(player);
            Ok(())
        }

        pub fn round_start(self, nth_decks: usize) -> Result<Table, &'static str> {
            if self.players.len() < 2 {
                panic!("Minimum number of players not reached.");
            }

            Table::new(self, nth_decks)
        }
    }

    /**
     * The table is where the game is played.
     */
    pub struct Table {
        pub deck: Arc<Mutex<Deck>>,
        pub players_with_hand: Vec<PlayerHand>,
        game: Game,
    }

    impl Table {
        fn new(game: Game, nth_decks: usize) -> Result<Self, &'static str> {
            // let bets = Vec::new();
            let mut players_with_hand = Vec::new();
            let deck = Deck::new_with_capacity(nth_decks)?;
            let deck = Arc::new(Mutex::new(deck));

            for player in game.players.iter() {
                let player_hand = PlayerHand::new(player.clone(), deck.clone());
                players_with_hand.push(player_hand);
            }

            // @TODO: Implement bet.

            Ok(Table {
                deck,
                players_with_hand,
                game,
            })
        }

        pub fn drop_table(self) -> Game {
            self.game
        }

        pub fn any_player_can_hit(&self) -> bool {
            self.players_with_hand
                .iter()
                .any(|player| !player.is_standing)
        }
    }
}
