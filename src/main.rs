//! An Asteroids-ish example game to show off ggez.
//! The idea is that this game is simple but still
//! non-trivial enough to be interesting.


extern crate ggez;
extern crate rand;
extern crate recs;

use ggez::audio;
use ggez::conf;
use ggez::event;
use ggez::graphics;
use ggez::graphics::Point2;
use ggez::nalgebra as na;
use ggez::{Context, ContextBuilder, GameResult};

use recs::{Ecs, EntityId};

use std::env;
use std::path;

mod event_loop;
mod vec;

use self::event_loop::{MainState, Tag, Transform, Physics, BoundingBox, Health, ShotLifetime};
use self::vec::{random_vec, vec_from_angle};

/// *********************************************************************
/// Now we define our Actor's.
/// An Actor is anything in the game world.
/// We're not *quite* making a real entity-component system but it's
/// pretty close.  For a more complicated game you would want a
/// real ECS, but for this it's enough to say that all our game objects
/// contain pretty much the same data.
/// **********************************************************************
#[derive(Debug, Clone)]
pub enum ActorType {
    Player,
    Rock,
    Shot,
}

/*
#[derive(Debug)]
pub struct Actor {
    tag: ActorType,
    pos: Point2,
    facing: f32,
    velocity: Vector2,
    ang_vel: f32,
    bbox_size: f32,

    // I am going to lazily overload "life" with a
    // double meaning:
    // for shots, it is the time left to live,
    // for players and rocks, it is the actual hit points.
    life: f32,
}
*/

pub const PLAYER_LIFE: f32 = 1.0;
pub const SHOT_LIFE: f32 = 2.0;
pub const ROCK_LIFE: f32 = 1.0;

pub const PLAYER_BBOX: f32 = 12.0;
pub const ROCK_BBOX: f32 = 12.0;
pub const SHOT_BBOX: f32 = 6.0;

pub const MAX_ROCK_VEL: f32 = 50.0;

/// *********************************************************************
/// Now we have some constructor functions for different game objects.
/// **********************************************************************

pub fn create_player(system: &mut Ecs) -> EntityId {
    let actor = system.create_entity();
    system.set(actor, Tag {
        tag: ActorType::Player,
    }).unwrap();

    system.set(actor, Transform {
        pos: Point2::origin(),
        facing: 0.,
    }).unwrap();

    system.set(actor, Physics {
        velocity: na::zero(),
        ang_vel: 0.,
    }).unwrap();

    system.set(actor, BoundingBox {
        bbox_size: PLAYER_BBOX,
    }).unwrap();

    system.set(actor, Health {
        health: PLAYER_LIFE,
    }).unwrap();

    actor
}

pub fn create_rock(system: &mut Ecs) -> EntityId {
    let actor = system.create_entity();

    system.set(actor, Tag {
        tag: ActorType::Rock,
    }).unwrap();

    system.set(actor, Transform {
        pos: Point2::origin(),
        facing: 0.,
    }).unwrap();

    system.set(actor, Physics {
        velocity: na::zero(),
        ang_vel: 0.,
    }).unwrap();

    system.set(actor, BoundingBox {
        bbox_size: ROCK_BBOX,
    }).unwrap();

    system.set(actor, Health {
        health: ROCK_LIFE
    }).unwrap();

    actor
}

pub fn create_shot(system: &mut Ecs) -> EntityId {
    let actor = system.create_entity();
    
    system.set(actor, Tag {
        tag: ActorType::Shot,
    }).unwrap();

    system.set(actor, Transform {
        pos: Point2::origin(),
        facing: 0.,
    }).unwrap();

    system.set(actor, Physics {
        velocity: na::zero(),
        ang_vel: SHOT_ANG_VEL,
    }).unwrap();

    system.set(actor, BoundingBox {
        bbox_size: SHOT_BBOX,
    }).unwrap();

    system.set(actor, ShotLifetime {
        time: SHOT_LIFE,
    }).unwrap();

    actor
}

/// Create the given number of rocks.
/// Makes sure that none of them are within the
/// given exclusion zone (nominally the player)
/// Note that this *could* create rocks outside the
/// bounds of the playing field, so it should be
/// called before `wrap_actor_position()` happens.
pub fn create_rocks(system: &mut Ecs, num: i32, exclusion: Point2, min_radius: f32, max_radius: f32) -> Vec<EntityId> {
    assert!(max_radius > min_radius);
    let new_rock = |_| {
        let rock = create_rock(system);
        let r_angle = rand::random::<f32>() * 2.0 * std::f32::consts::PI;
        let r_distance = rand::random::<f32>() * (max_radius - min_radius) + min_radius;
        
        let transfrom: &mut Transform = system.borrow_mut(rock).unwrap();
        transfrom.pos = exclusion + vec_from_angle(r_angle) * r_distance;

        let physics: &mut Physics = system.borrow_mut(rock).unwrap();
        physics.velocity = random_vec(MAX_ROCK_VEL);

        rock
    };
    (0..num).map(new_rock).collect()
}

/// *********************************************************************
/// Now we make functions to handle physics.  We do simple Newtonian
/// physics (so we do have inertia), and cap the max speed so that we
/// don't have to worry too much about small objects clipping through
/// each other.
///
/// Our unit of world space is simply pixels, though we do transform
/// the coordinate system so that +y is up and -y is down.
/// **********************************************************************

pub const SHOT_SPEED: f32 = 200.0;
pub const SHOT_ANG_VEL: f32 = 0.1;

// Acceleration in pixels per second.
pub const PLAYER_THRUST: f32 = 100.0;
// Rotation in radians per second.
pub const PLAYER_TURN_RATE: f32 = 3.0;
// Seconds between shots
pub const PLAYER_SHOT_TIME: f32 = 0.5;

pub fn player_handle_input(system: &mut Ecs, actor: EntityId, input: &InputState, dt: f32) {
    let mut transform: Transform = system.get(actor).unwrap();
    let mut physics: Physics = system.get(actor).unwrap();

    transform.facing += dt * PLAYER_TURN_RATE * input.xaxis;

    if input.yaxis > 0.0 {
        player_thrust(&mut transform, &mut physics, dt);
    }

    system.set(actor, transform).unwrap();
    system.set(actor, physics).unwrap();
}

pub fn player_thrust(transform: &mut Transform, physics: &mut Physics, dt: f32) {
    let direction_vector = vec_from_angle(transform.facing);
    let thrust_vector = direction_vector * (PLAYER_THRUST);
    physics.velocity += thrust_vector * (dt);
}

pub const MAX_PHYSICS_VEL: f32 = 250.0;

pub fn update_actor_position(system: &mut Ecs, actor: EntityId, dt: f32) {
    let mut transform: Transform = system.get(actor).unwrap();
    let mut physics: Physics = system.get(actor).unwrap();

    // Clamp the velocity to the max efficiently
    let norm_sq = physics.velocity.norm_squared();
    if norm_sq > MAX_PHYSICS_VEL.powi(2) {
        physics.velocity = physics.velocity / norm_sq.sqrt() * MAX_PHYSICS_VEL;
    }
    let dv = physics.velocity * (dt);
    transform.pos += dv;
    transform.facing += physics.ang_vel;

    system.set(actor, transform).unwrap();
    system.set(actor, physics).unwrap();
}

/// Takes an actor and wraps its position to the bounds of the
/// screen, so if it goes off the left side of the screen it
/// will re-enter on the right side and so on.
pub fn wrap_actor_position(system: &mut Ecs, actor: EntityId, sx: f32, sy: f32) {
    let transform: &mut Transform = system.borrow_mut(actor).unwrap();
    
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

pub fn handle_shot_timer(system: &mut Ecs, actor: EntityId, dt: f32) {
    let lifetime: &mut ShotLifetime = system.borrow_mut(actor).unwrap();
    lifetime.time -= dt;
}

/// Translates the world coordinate system, which
/// has Y pointing up and the origin at the center,
/// to the screen coordinate system, which has Y
/// pointing downward and the origin at the top-left,
pub fn world_to_screen_coords(screen_width: u32, screen_height: u32, point: Point2) -> Point2 {
    let width = screen_width as f32;
    let height = screen_height as f32;
    let x = point.x + width / 2.0;
    let y = height - (point.y + height / 2.0);
    Point2::new(x, y)
}

/// **********************************************************************
/// So that was the real meat of our game.  Now we just need a structure
/// to contain the images, sounds, etc. that we need to hang on to; this
/// is our "asset management system".  All the file names and such are
/// just hard-coded.
/// **********************************************************************

pub struct Assets {
    player_image: graphics::Image,
    shot_image: graphics::Image,
    rock_image: graphics::Image,
    font: graphics::Font,
    shot_sound: audio::Source,
    hit_sound: audio::Source,
}

impl Assets {
    pub fn new(ctx: &mut Context) -> GameResult<Assets> {
        let player_image = graphics::Image::new(ctx, "/player.png")?;
        let shot_image = graphics::Image::new(ctx, "/shot.png")?;
        let rock_image = graphics::Image::new(ctx, "/rock.png")?;
        let font = graphics::Font::new(ctx, "/DejaVuSerif.ttf", 18)?;

        let shot_sound = audio::Source::new(ctx, "/pew.ogg")?;
        let hit_sound = audio::Source::new(ctx, "/boom.ogg")?;
        Ok(Assets {
            player_image,
            shot_image,
            rock_image,
            font,
            shot_sound,
            hit_sound,
        })
    }

    pub fn actor_image(&mut self, system: &mut Ecs, actor: EntityId) -> &mut graphics::Image {
        match system.get::<Tag>(actor).unwrap().tag {
            ActorType::Player => &mut self.player_image,
            ActorType::Rock => &mut self.rock_image,
            ActorType::Shot => &mut self.shot_image,
        }
    }
}

/// **********************************************************************
/// The `InputState` is exactly what it sounds like, it just keeps track of
/// the user's input state so that we turn keyboard events into something
/// state-based and device-independent.
/// **********************************************************************
#[derive(Debug)]
pub struct InputState {
    xaxis: f32,
    yaxis: f32,
    fire: bool,
}

impl Default for InputState {
    fn default() -> Self {
        InputState {
            xaxis: 0.0,
            yaxis: 0.0,
            fire: false,
        }
    }
}



/// **********************************************************************
/// A couple of utility functions.
/// **********************************************************************

pub fn print_instructions() {
    println!();
    println!("Welcome to ASTROBLASTO!");
    println!();
    println!("How to play:");
    println!("L/R arrow keys rotate your ship, up thrusts, space bar fires");
    println!();
}

pub fn draw_actor(
    assets: &mut Assets,
    ctx: &mut Context,
    system: &mut Ecs,
    actor: EntityId,
    world_coords: (u32, u32),
) -> GameResult<()> {
    let transform: &Transform = system.borrow(actor).unwrap();
    let (screen_w, screen_h) = world_coords;
    let pos = world_to_screen_coords(screen_w, screen_h, transform.pos);
    let drawparams = graphics::DrawParam {
        dest: pos,
        rotation: transform.facing as f32,
        offset: graphics::Point2::new(0.5, 0.5),
        ..Default::default()
    };
    let image = assets.actor_image(system, actor);
    graphics::draw_ex(ctx, image, drawparams)
}


/// **********************************************************************
/// Finally our main function!  Which merely sets up a config and calls
/// `ggez::event::run()` with our `EventHandler` type.
/// **********************************************************************

pub fn main() {
    let mut cb = ContextBuilder::new("astroblasto", "ggez")
        .window_setup(conf::WindowSetup::default().title("Astroblasto!"))
        .window_mode(conf::WindowMode::default().dimensions(640, 480));

    // We add the CARGO_MANIFEST_DIR/resources to the filesystems paths so
    // we we look in the cargo project for files.
    if let Ok(manifest_dir) = env::var("CARGO_MANIFEST_DIR") {
        let mut path = path::PathBuf::from(manifest_dir);
        path.push("resources");
        println!("Adding path {:?}", path);
        // We need this re-assignment alas, see
        // https://aturon.github.io/ownership/builders.html
        // under "Consuming builders"
        cb = cb.add_resource_path(path);
    } else {
        println!("Not building from cargo?  Ok.");
    }

    let ctx = &mut cb.build().unwrap();

    match MainState::new(ctx) {
        Err(e) => {
            println!("Could not load game!");
            println!("Error: {}", e);
        }
        Ok(ref mut game) => {
            let result = event::run(ctx, game);
            if let Err(e) = result {
                println!("Error encountered running game: {}", e);
            } else {
                println!("Game exited cleanly.");
            }
        }
    }
}