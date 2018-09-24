use ggez::event::{EventHandler, Keycode, Mod};
use ggez::graphics;
use ggez::timer;
use ggez::{Context, GameResult};

use super::better_ecs::{Ecs, EntityId};
use super::components::{
    Health, Physics, Player, Rock, ShotLifetime, Sprite, Transform, Collider
};

use super::prefabs::{create_player, create_rocks};

use super::{print_instructions, Assets, InputState};

/// **********************************************************************
/// Now we're getting into the actual game loop.  The `MainState` is our
/// game's "global" state, it keeps track of everything we need for
/// actually running the game.
///
/// Our game objects are simply a vector for each actor type, and we
/// probably mingle gameplay-state (like score) and hardware-state
/// (like `gui_dirty`) a little more than we should, but for something
/// this small it hardly matters.
/// **********************************************************************

type Score = u32;

pub struct MainState {
    player: EntityId,
    level: i32,
    score: Score,
    assets: Assets,
    screen_width: u32,
    screen_height: u32,
    input: InputState,
    gui_dirty: bool,
    score_display: graphics::Text,
    level_display: graphics::Text,

    system: Ecs,
}

impl MainState {
    pub fn new(ctx: &mut Context) -> GameResult<MainState> {
        ctx.print_resource_stats();
        graphics::set_background_color(ctx, (0, 0, 0, 255).into());

        println!("Game resource path: {:?}", ctx.filesystem);

        print_instructions();

        let mut entity_system = Ecs::new();

        let assets = Assets::new(ctx)?;
        let score_disp = graphics::Text::new(ctx, "score", &assets.font)?;
        let level_disp = graphics::Text::new(ctx, "level", &assets.font)?;

        let player = create_player(&mut entity_system);
        let player_transform: Transform = entity_system.get(player).unwrap();
        create_rocks(&mut entity_system, 5, player_transform.pos, 100.0, 250.0);

        let s = MainState {
            player,
            level: 0,
            score: 0,
            assets,
            screen_width: ctx.conf.window_mode.width,
            screen_height: ctx.conf.window_mode.height,
            input: InputState::default(),
            gui_dirty: true,
            score_display: score_disp,
            level_display: level_disp,

            system: entity_system,
        };

        Ok(s)
    }

    pub fn clear_dead_stuff(&mut self) {
        let mut removals =
            self.system
                .components_ref::<Health>()
                .filter(|(id, actor)| {
                    self.system.get_parent(*id).unwrap() != self.player && actor.health <= 0.0
                }).map(|(id, _)| self.system.get_parent(id).unwrap())
                .collect::<Vec<_>>();

        let score_increase = removals.len() as Score;
        if score_increase != 0 {
            self.score += score_increase;
            self.gui_dirty = true;
        }

        removals.extend(
            self.system
            .components_ref::<ShotLifetime>()
            .filter(|(_, shot)| shot.time <= 0.0)
            .map(|(id, _)| self.system.get_parent(id).unwrap())
            .collect::<Vec<_>>()
        );

        for id in removals {
            self.system.remove_entity(id).unwrap();
        }
    }

    pub fn check_for_level_respawn(&mut self) {
        if self.system.entities_with::<Rock>().is_empty() {
            let transform: Transform = self.system.get(self.player).unwrap();

            self.level += 1;
            self.gui_dirty = true;
            create_rocks(
                &mut self.system,
                self.level + 5,
                transform.pos,
                100.0,
                250.0,
            );
        }
    }

    pub fn update_ui(&mut self, ctx: &mut Context) {
        let score_str = format!("Score: {}", self.score);
        let level_str = format!("Level: {}", self.level);
        let score_text = graphics::Text::new(ctx, &score_str, &self.assets.font).unwrap();
        let level_text = graphics::Text::new(ctx, &level_str, &self.assets.font).unwrap();

        self.score_display = score_text;
        self.level_display = level_text;
    }
}

/// **********************************************************************
/// Now we implement the `EventHandler` trait from `ggez::event`, which provides
/// ggez with callbacks for updating and drawing our game, as well as
/// handling input events.
/// **********************************************************************
impl EventHandler for MainState {
    fn update(&mut self, ctx: &mut Context) -> GameResult<()> {
        const DESIRED_FPS: u32 = 60;

        while timer::check_update_time(ctx, DESIRED_FPS) {
            let seconds = 1.0 / (DESIRED_FPS as f32);

            // Update the player state based on the user input.
            let mut new_shots = Ecs::empty();
            self.system
                .components_mut::<Player>()
                .for_each(|(_, mut player)| {
                    player.player_handle_input(&self.system, &self.input, seconds);
                    player.try_fire(
                        &self.system,
                        &mut new_shots,
                        &self.input,
                        &self.assets,
                        seconds,
                    );
                });
            self.system.merge(new_shots);

            // Update the physics for all actors.
            self.system
                .components_mut::<Physics>()
                .for_each(|(_, mut component)| {
                    component.update_actor_position(&self.system, seconds);
                    component.wrap_actor_position(
                        &self.system,
                        self.screen_width as f32,
                        self.screen_height as f32,
                    )
                });

            // Update the timers for shots.
            self.system
                .components_mut::<ShotLifetime>()
                .for_each(|(_, mut shot)| {
                    shot.handle_shot_timer(seconds);
                });

            // Handle the results of things moving:
            // collision detection, object death, and if
            // we have killed all the rocks in the level,
            // spawn more of them.
            self.system.components_ref::<Collider>()
                .for_each(|(_, collider)| {
                    collider.check_for_collisions(&self.system, &self.assets);
                });

            self.clear_dead_stuff();

            self.check_for_level_respawn();

            // Using a gui_dirty flag here is a little
            // messy but fine here.
            if self.gui_dirty {
                self.update_ui(ctx);
                self.gui_dirty = false;
            }

            // Finally we check for our end state.
            // I want to have a nice death screen eventually,
            // but for now we just quit.
            let player_health: Health = self.system.get(self.player).unwrap();
            if player_health.health <= 0.0 {
                println!("Game over!");
                let _ = ctx.quit();
            }
        }

        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult<()> {
        // Our drawing is quite simple.
        // Just clear the screen...
        graphics::clear(ctx);

        // Loop over all objects drawing them...
        let coords = (self.screen_width, self.screen_height);
        for (_, sprite) in self.system.components_ref::<Sprite>() {
            sprite
                .draw_actor(&self.assets, ctx, &self.system, coords)
                .unwrap();
        }

        // And draw the GUI elements in the right places.
        let level_dest = graphics::Point2::new(10.0, 10.0);
        let score_dest = graphics::Point2::new(200.0, 10.0);
        graphics::draw(ctx, &self.level_display, level_dest, 0.0)?;
        graphics::draw(ctx, &self.score_display, score_dest, 0.0)?;

        // Then we flip the screen...
        graphics::present(ctx);

        // And yield the timeslice
        // This tells the OS that we're done using the CPU but it should
        // get back to this program as soon as it can.
        // This ideally prevents the game from using 100% CPU all the time
        // even if vsync is off.
        // The actual behavior can be a little platform-specific.
        timer::yield_now();
        Ok(())
    }

    // Handle key events.  These just map keyboard events
    // and alter our input state appropriately.
    fn key_down_event(&mut self, ctx: &mut Context, keycode: Keycode, _keymod: Mod, _repeat: bool) {
        match keycode {
            Keycode::Up => {
                self.input.yaxis = 1.0;
            }
            Keycode::Left => {
                self.input.xaxis = -1.0;
            }
            Keycode::Right => {
                self.input.xaxis = 1.0;
            }
            Keycode::Space => {
                self.input.fire = true;
            }
            Keycode::P => {
                let img = graphics::screenshot(ctx).expect("Could not take screenshot");
                img.encode(ctx, graphics::ImageFormat::Png, "/screenshot.png")
                    .expect("Could not save screenshot");
            }
            Keycode::Escape => ctx.quit().unwrap(),
            _ => (), // Do nothing
        }
    }

    fn key_up_event(&mut self, _ctx: &mut Context, keycode: Keycode, _keymod: Mod, _repeat: bool) {
        match keycode {
            Keycode::Up => {
                self.input.yaxis = 0.0;
            }
            Keycode::Left | Keycode::Right => {
                self.input.xaxis = 0.0;
            }
            Keycode::Space => {
                self.input.fire = false;
            }
            _ => (), // Do nothing
        }
    }
}
