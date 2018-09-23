use ggez::event::{EventHandler, Keycode, Mod};
use ggez::graphics::{self, Point2, Vector2};
use ggez::timer;
use ggez::{Context, GameResult};


use super::better_ecs::{Ecs, EntityId, ComponentId};
use super::ActorType;
use super::MAX_PHYSICS_VEL;

use ggez::nalgebra as na;

use super::{
    create_player, create_rocks, create_shot, draw_actor, handle_shot_timer, player_handle_input,
    print_instructions, vec_from_angle, Assets,
    InputState, PLAYER_SHOT_TIME, SHOT_SPEED,
};

// Components.
#[derive(Clone)]
pub struct Tag {
    pub tag: ActorType,
}

#[derive(Clone)]
pub struct Transform {
    pub pos: Point2,
    pub facing: f32,
}

impl Default for Transform {
    fn default() -> Self {
        Transform {
            pos: Point2::origin(),
            facing: 0.0
        }
    }
}

#[derive(Clone)]
pub struct Physics {
    pub velocity: Vector2,
    pub ang_vel: f32,

    pub transform: ComponentId
}

impl Physics {
    pub fn new(transform: ComponentId) -> Self {
        Physics {
            velocity: na::zero(),
            ang_vel: 0.0,
            transform
        }
    }

    pub fn update_actor_position(&mut self, system: &Ecs, dt: f32) {
        let mut transform = system.borrow_mut_by_id::<Transform>(self.transform).unwrap();

        // Clamp the velocity to the max efficiently
        let norm_sq = self.velocity.norm_squared();
        if norm_sq > MAX_PHYSICS_VEL.powi(2) {
            self.velocity = self.velocity / norm_sq.sqrt() * MAX_PHYSICS_VEL;
        }
        let dv = self.velocity * (dt);
        transform.pos += dv;
        transform.facing += self.ang_vel;
    }

    /// Takes an actor and wraps its position to the bounds of the
    /// screen, so if it goes off the left side of the screen it
    /// will re-enter on the right side and so on.
    pub fn wrap_actor_position(&mut self, system: &Ecs, sx: f32, sy: f32) {
        let mut transform = system.borrow_mut_by_id::<Transform>(self.transform).unwrap();

        // Wrap screen
        let screen_x_bounds = sx / 2.0;
        let screen_y_bounds = sy / 2.0;
        if transform.pos.x > screen_x_bounds {
            transform.pos.x -= sx;
        } else if transform.pos.x < -screen_x_bounds {
            transform.pos.x += sx;
        };
        if transform.pos.y > screen_y_bounds {
            transform.pos.y -= sy;
        } else if transform.pos.y < -screen_y_bounds {
            transform.pos.y += sy;
        }
    }
}

#[derive(Clone)]
pub struct BoundingBox {
    pub bbox_size: f32,

    pub transform: ComponentId
}

impl BoundingBox {
    pub fn new(bbox_size: f32, transform: ComponentId) -> Self {
        BoundingBox {
            bbox_size,
            transform
        }
    }
}

#[derive(Clone)]
pub struct Health {
    pub health: f32,
}

#[derive(Clone)]
pub struct ShotLifetime {
    pub time: f32,
}

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

pub struct MainState {
    player: EntityId,
    shots: Vec<EntityId>,
    rocks: Vec<EntityId>,
    level: i32,
    score: i32,
    assets: Assets,
    screen_width: u32,
    screen_height: u32,
    input: InputState,
    player_shot_timeout: f32,
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
        let rocks = create_rocks(&mut entity_system, 5, player_transform.pos, 100.0, 250.0);

        let s = MainState {
            player,
            shots: Vec::new(),
            rocks,
            level: 0,
            score: 0,
            assets,
            screen_width: ctx.conf.window_mode.width,
            screen_height: ctx.conf.window_mode.height,
            input: InputState::default(),
            player_shot_timeout: 0.0,
            gui_dirty: true,
            score_display: score_disp,
            level_display: level_disp,

            system: entity_system,
        };

        Ok(s)
    }

    pub fn fire_player_shot(&mut self) {
        self.player_shot_timeout = PLAYER_SHOT_TIME;

        let shot = create_shot(&mut self.system);

        let player_transform: Transform = self.system.get(self.player).unwrap();
        let direction;
        {
            let mut shot_transform = self.system.borrow_mut::<Transform>(shot).unwrap();
            shot_transform.pos = player_transform.pos;
            shot_transform.facing = player_transform.facing;
            direction = vec_from_angle(shot_transform.facing);
        }

        {
            let mut shot_physics = self.system.borrow_mut::<Physics>(shot).unwrap();
            shot_physics.velocity.x = SHOT_SPEED * direction.x;
            shot_physics.velocity.y = SHOT_SPEED * direction.y;
        }

        self.shots.push(shot);
        let _ = self.assets.shot_sound.play();
    }

    pub fn clear_dead_stuff(&mut self) {
        let mut shots = Vec::with_capacity(0);
        let mut rocks = Vec::with_capacity(0);
        std::mem::swap(&mut self.shots, &mut shots);
        std::mem::swap(&mut self.rocks, &mut rocks);

        shots.retain(|&s| self.system.get::<ShotLifetime>(s).unwrap().time > 0.0);
        rocks.retain(|&r| self.system.get::<Health>(r).unwrap().health > 0.0);

        self.shots = shots;
        self.rocks = rocks;
    }

    pub fn handle_collisions(&mut self) {
        for &rock in &self.rocks {
            let rock_transform: Transform = self.system.get(rock).unwrap();
            let rock_bbox: BoundingBox = self.system.get(rock).unwrap();
            let player_transform: Transform = self.system.get(self.player).unwrap();
            let player_bbox: BoundingBox = self.system.get(self.player).unwrap();

            let pdistance = rock_transform.pos - player_transform.pos;
            if pdistance.norm() < (player_bbox.bbox_size + rock_bbox.bbox_size) {
                self.system
                    .set(self.player, Health { health: 0.0 })
                    .unwrap();
            }
            for &shot in &self.shots {
                let shot_transform: Transform = self.system.get(shot).unwrap();
                let shot_bbox: BoundingBox = self.system.get(shot).unwrap();

                let distance = shot_transform.pos - rock_transform.pos;
                if distance.norm() < (shot_bbox.bbox_size + rock_bbox.bbox_size) {
                    self.system.set(shot, ShotLifetime { time: 0.0 }).unwrap();
                    self.system.set(rock, Health { health: 0.0 }).unwrap();
                    self.score += 1;
                    self.gui_dirty = true;
                    let _ = self.assets.hit_sound.play();
                }
            }
        }
    }

    pub fn check_for_level_respawn(&mut self) {
        if self.rocks.is_empty() {
            let transform: Transform = self.system.get(self.player).unwrap();

            self.level += 1;
            self.gui_dirty = true;
            let r = create_rocks(
                &mut self.system,
                self.level + 5,
                transform.pos,
                100.0,
                250.0,
            );
            self.rocks.extend(r);
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
            player_handle_input(&mut self.system, self.player, &self.input, seconds);
            self.player_shot_timeout -= seconds;
            if self.input.fire && self.player_shot_timeout < 0.0 {
                self.fire_player_shot();
            }

            // Update the physics for all actors.
            self.system.components_mut::<Physics>().for_each(|mut component| {
                component.update_actor_position(&self.system, seconds);
                component.wrap_actor_position(
                    &self.system,
                    self.screen_width as f32,
                    self.screen_height as f32,
                )
            });

            for &act in &self.shots {
                handle_shot_timer(&mut self.system, act, seconds);
            }

            // Handle the results of things moving:
            // collision detection, object death, and if
            // we have killed all the rocks in the level,
            // spawn more of them.
            self.handle_collisions();

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
        {
            let assets = &mut self.assets;
            let coords = (self.screen_width, self.screen_height);

            draw_actor(assets, ctx, &mut self.system, self.player, coords)?;

            for &s in &self.shots {
                draw_actor(assets, ctx, &mut self.system, s, coords)?;
            }

            for &r in &self.rocks {
                draw_actor(assets, ctx, &mut self.system, r, coords)?;
            }
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
