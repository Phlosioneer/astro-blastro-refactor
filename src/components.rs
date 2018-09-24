use ggez::graphics::{self, Point2, Vector2};
use ggez::nalgebra as na;
use ggez::{Context, GameResult};

use super::better_ecs::{ComponentRef, Ecs};
use super::prefabs::create_shot;
use super::vec::vec_from_angle;
use super::world_to_screen_coords;
use super::{Assets, InputState};
use super::{MAX_PHYSICS_VEL, SHOT_SPEED};

#[derive(Debug, Clone)]
pub enum ActorType {
    Player,
    Rock,
    Shot,
}

#[derive(Clone)]
pub struct Player {
    pub player_shot_timeout: f32,
    pub transform: ComponentRef<Transform>,
    pub physics: ComponentRef<Physics>,
}

// Acceleration in pixels per second.
pub const PLAYER_THRUST: f32 = 100.0;
// Rotation in radians per second.
pub const PLAYER_TURN_RATE: f32 = 3.0;
// Seconds between shots
pub const PLAYER_SHOT_TIME: f32 = 0.5;

impl Player {
    pub fn new(transform: ComponentRef<Transform>, physics: ComponentRef<Physics>) -> Self {
        Player {
            player_shot_timeout: PLAYER_SHOT_TIME,
            transform: transform.into(),
            physics: physics.into(),
        }
    }

    pub fn player_handle_input(&mut self, system: &Ecs, input: &InputState, dt: f32) {
        let mut transform = self.transform.borrow_mut(system).unwrap();

        transform.facing += dt * PLAYER_TURN_RATE * input.xaxis;

        drop(transform);

        if input.yaxis > 0.0 {
            self.player_thrust(system, dt);
        }
    }

    pub fn player_thrust(&mut self, system: &Ecs, dt: f32) {
        let transform = self.transform.borrow(system).unwrap();
        let mut physics = self.physics.borrow_mut(system).unwrap();
        let direction_vector = vec_from_angle(transform.facing);
        let thrust_vector = direction_vector * (PLAYER_THRUST);
        physics.velocity += thrust_vector * (dt);
    }

    pub fn try_fire(
        &mut self,
        system: &Ecs,
        new_shots_ecs: &mut Ecs,
        input: &InputState,
        assets: &Assets,
        dt: f32,
    ) {
        self.player_shot_timeout -= dt;
        if input.fire && self.player_shot_timeout < 0.0 {
            self.fire_player_shot(system, new_shots_ecs, assets);
        }
    }

    pub fn fire_player_shot(&mut self, system: &Ecs, new_shots_ecs: &mut Ecs, assets: &Assets) {
        self.player_shot_timeout = PLAYER_SHOT_TIME;

        let shot = create_shot(new_shots_ecs);
        let mut shot_transform = new_shots_ecs.borrow_mut::<Transform>(shot).unwrap();
        let mut shot_physics = new_shots_ecs.borrow_mut::<Physics>(shot).unwrap();

        let player_transform = self.transform.borrow(system).unwrap();
        shot_transform.pos = player_transform.pos;
        shot_transform.facing = player_transform.facing;
        let direction = vec_from_angle(shot_transform.facing);

        shot_physics.velocity.x = SHOT_SPEED * direction.x;
        shot_physics.velocity.y = SHOT_SPEED * direction.y;

        // TODO: self.shots.push(shot);
        assets.shot_sound.play().unwrap();
    }
}

#[derive(Clone)]
pub struct Tag {
    pub tag: ActorType,
}

impl Tag {
    pub fn new(tag: ActorType) -> Tag {
        Tag {
            tag
        }
    }
}

#[derive(Clone)]
pub struct Rock;

#[derive(Clone)]
pub struct Transform {
    pub pos: Point2,
    pub facing: f32,
}

impl Default for Transform {
    fn default() -> Self {
        Transform {
            pos: Point2::origin(),
            facing: 0.0,
        }
    }
}

#[derive(Clone)]
pub struct Physics {
    pub velocity: Vector2,
    pub ang_vel: f32,

    pub transform: ComponentRef<Transform>,
}

impl Physics {

    pub fn new(transform: ComponentRef<Transform>) -> Self {
        Physics {
            velocity: na::zero(),
            ang_vel: 0.0,
            transform,
        }
    }

    pub fn update_actor_position(&mut self, system: &Ecs, dt: f32) {
        let mut transform = self.transform.borrow_mut(system).unwrap();

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
        let mut transform = self.transform.borrow_mut(system).unwrap();

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

// Note: This is actually implemented as a bounding CIRCLE, not a box...
#[derive(Clone)]
pub struct BoundingBox {
    pub bbox_size: f32,

    pub transform: ComponentRef<Transform>,
}

impl BoundingBox {
    pub fn new(bbox_size: f32, transform: ComponentRef<Transform>) -> Self {
        BoundingBox {
            bbox_size,
            transform,
        }
    }

    pub fn is_touching(&self, system: &Ecs, other: &BoundingBox) -> bool {
        let transform = self.transform.borrow(system).unwrap();
        let other_transform = other.transform.borrow(system).unwrap();

        let pdistance = transform.pos - other_transform.pos;
        
        pdistance.norm() < (self.bbox_size + other.bbox_size)
    }
}

#[derive(Clone)]
pub struct Collider {
    pub bounds: ComponentRef<BoundingBox>,
    pub health: ComponentRef<Health>,
}

impl Collider {
    pub fn new(
            bounds: ComponentRef<BoundingBox>,
            health: ComponentRef<Health>) -> Collider
    {
        Collider {
            bounds,
            health
        }
    }

    pub fn check_for_collisions(&self, system: &Ecs, assets: &Assets) {
        let rock_bbox = self.bounds.borrow(system).unwrap();

        for player in system.entities_with::<Player>() {
            let player_bbox = system.get::<BoundingBox>(player).unwrap();

            if rock_bbox.is_touching(system, &player_bbox) {
                system.borrow_mut::<Health>(player).unwrap().health = 0.0;
            }
        }
        for shot in system.entities_with::<ShotLifetime>() {
            let shot_bbox = system.get::<BoundingBox>(shot).unwrap();

            if rock_bbox.is_touching(system, &shot_bbox) {
                system.borrow_mut::<ShotLifetime>(shot).unwrap().time = 0.0;
                self.health.borrow_mut(system).unwrap().health = 0.0;
                assets.hit_sound.play().unwrap();
            }
        }
    }
}

#[derive(Clone)]
pub struct Health {
    pub health: f32,
}

impl Health {
    pub fn new(health: f32) -> Health {
        Health { health }
    }
}

#[derive(Clone)]
pub struct ShotLifetime {
    pub time: f32,
}

impl ShotLifetime {
    pub fn new(time: f32) -> ShotLifetime {
        ShotLifetime { time }
    }

    pub fn handle_shot_timer(&mut self, dt: f32) {
        self.time -= dt;
    }
}

#[derive(Clone)]
pub struct Sprite {
    pub tag: ComponentRef<Tag>,
    pub transform: ComponentRef<Transform>,
}

impl Sprite {
    pub fn new(tag: ComponentRef<Tag>, transform: ComponentRef<Transform>) -> Self {
        Sprite {
            tag,
            transform,
        }
    }

    pub fn draw_actor(
        &self,
        assets: &Assets,
        ctx: &mut Context,
        system: &Ecs,
        world_coords: (u32, u32),
    ) -> GameResult<()> {
        let transform = self.transform.borrow(system).unwrap();
        let (screen_w, screen_h) = world_coords;
        let pos = world_to_screen_coords(screen_w, screen_h, transform.pos);
        let drawparams = graphics::DrawParam {
            dest: pos,
            rotation: transform.facing as f32,
            offset: graphics::Point2::new(0.5, 0.5),
            ..Default::default()
        };
        let tag = &self.tag.borrow(system).unwrap().tag;
        let image = assets.actor_image(tag);
        graphics::draw_ex(ctx, image, drawparams)
    }
}
