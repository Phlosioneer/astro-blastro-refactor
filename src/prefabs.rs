use ggez::graphics::Point2;

use super::better_ecs::{Ecs, EntityId};
use super::components::{
    ActorType, BoundingBox, Health, Physics, Player, Rock, ShotLifetime, Sprite, Tag, Transform,
};
use super::vec::{random_vec, vec_from_angle};
use super::MAX_ROCK_VEL;

pub const PLAYER_LIFE: f32 = 1.0;
pub const SHOT_LIFE: f32 = 2.0;
pub const ROCK_LIFE: f32 = 1.0;

pub const PLAYER_BBOX: f32 = 12.0;
pub const ROCK_BBOX: f32 = 12.0;
pub const SHOT_BBOX: f32 = 6.0;

/// *********************************************************************
/// Now we have some constructor functions for different game objects.
/// **********************************************************************

pub fn create_player(system: &mut Ecs) -> EntityId {
    let actor = system.create_entity();
    let tag = system
        .set(
            actor,
            Tag {
                tag: ActorType::Player,
            },
        ).unwrap();

    let transform = system.set(actor, Transform::default()).unwrap();

    let physics = system.set(actor, Physics::new(transform)).unwrap();

    system.set(actor, Sprite::new(tag, transform)).unwrap();

    system
        .set(actor, BoundingBox::new(PLAYER_BBOX, transform))
        .unwrap();

    system
        .set(
            actor,
            Health {
                health: PLAYER_LIFE,
            },
        ).unwrap();

    system.set(actor, Player::new(transform, physics)).unwrap();

    actor
}

pub fn create_rock(system: &mut Ecs) -> EntityId {
    let actor = system.create_entity();

    let tag = system
        .set(
            actor,
            Tag {
                tag: ActorType::Rock,
            },
        ).unwrap();

    system.set(actor, Rock).unwrap();

    let transform = system.set(actor, Transform::default()).unwrap();

    system.set(actor, Sprite::new(tag, transform)).unwrap();

    system.set(actor, Physics::new(transform)).unwrap();

    system
        .set(actor, BoundingBox::new(ROCK_BBOX, transform))
        .unwrap();

    system.set(actor, Health { health: ROCK_LIFE }).unwrap();

    actor
}

pub fn create_shot(system: &mut Ecs) -> EntityId {
    let actor = system.create_entity();

    let tag = system
        .set(
            actor,
            Tag {
                tag: ActorType::Shot,
            },
        ).unwrap();

    let transform = system.set(actor, Transform::default()).unwrap();

    system.set(actor, Physics::new(transform)).unwrap();

    system.set(actor, Sprite::new(tag, transform)).unwrap();

    system
        .set(actor, BoundingBox::new(SHOT_BBOX, transform))
        .unwrap();

    system.set(actor, ShotLifetime { time: SHOT_LIFE }).unwrap();

    actor
}

/// Create the given number of rocks.
/// Makes sure that none of them are within the
/// given exclusion zone (nominally the player)
/// Note that this *could* create rocks outside the
/// bounds of the playing field, so it should be
/// called before `wrap_actor_position()` happens.
pub fn create_rocks(
    system: &mut Ecs,
    num: i32,
    exclusion: Point2,
    min_radius: f32,
    max_radius: f32,
) -> Vec<EntityId> {
    assert!(max_radius > min_radius);
    let new_rock = |_| {
        let rock = create_rock(system);
        let r_angle = rand::random::<f32>() * 2.0 * std::f32::consts::PI;
        let r_distance = rand::random::<f32>() * (max_radius - min_radius) + min_radius;

        let mut transfrom = system.borrow_mut::<Transform>(rock).unwrap();
        transfrom.pos = exclusion + vec_from_angle(r_angle) * r_distance;

        let mut physics = system.borrow_mut::<Physics>(rock).unwrap();
        physics.velocity = random_vec(MAX_ROCK_VEL);

        rock
    };
    (0..num).map(new_rock).collect()
}
