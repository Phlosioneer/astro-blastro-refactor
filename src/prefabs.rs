use ggez::graphics::Point2;

use super::better_ecs::{Ecs, EntityId};
use super::components::{
    ActorType, Collider, BoundingBox, Health, Physics, Player, Rock, ShotLifetime, Sprite, Tag, Transform,
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
    system.build_entity()
        .with(Tag::new(ActorType::Player))
        .with(Transform::default())
        .with1(Physics::new)
        .with2(Sprite::new)
        .with1(|transform| BoundingBox::new(PLAYER_BBOX, transform))
        .with(Health::new(PLAYER_LIFE))
        .with2(Player::new)
        .build()
        .unwrap()
}

pub fn create_rock(system: &mut Ecs) -> EntityId {
    system.build_entity()
        .with(Tag::new(ActorType::Rock))
        .with(Transform::default())
        .with1(Physics::new)
        .with2(Sprite::new)
        .with1(|transform| BoundingBox::new(ROCK_BBOX, transform))
        .with(Health::new(ROCK_LIFE))
        .with(Rock)
        .with2(Collider::new)
        .build()
        .unwrap()
}

pub fn create_shot(system: &mut Ecs) -> EntityId {
    system.build_entity()
        .with(Tag::new(ActorType::Shot))
        .with(Transform::default())
        .with1(Physics::new)
        .with2(Sprite::new)
        .with1(|transform| BoundingBox::new(SHOT_BBOX, transform))
        .with(ShotLifetime::new(SHOT_LIFE))
        .build()
        .unwrap()
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
