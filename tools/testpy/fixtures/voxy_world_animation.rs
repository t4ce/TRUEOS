mod scene { pub mod figure {
    use anim::{Animation, character::CharacterSkeleton};
    use common::{comp::{self, CharacterState, CharacterActivity, Inventory, PhysicsState, PoiseState,
        inventory::slot::EquipSlot, item::ItemKind}, states::{idle, equipping, interact, wielding, utils::StageSection},
        interaction::InteractionKind, link::Is, mounting::VolumeRider, terrain::SpriteKind,
        util::{Dir, div::checked_div}};
    use vek::*;
    #[path = "@VOXY@/src/scene/figure/character_pose.rs"] pub mod character_pose;
} }
#[path = "@VOXY@/src/render/world_animation.rs"] mod world_animation;
use common::{comp::{self, CharacterState, PhysicsState}, util::Dir};
use vek::*;
fn body() -> comp::humanoid::Body {
    comp::humanoid::Body { species: comp::humanoid::Species::Human, body_type: comp::humanoid::BodyType::Male,
        hair_style:0, beard:0, eyes:0, accessory:0, hair_color:0, skin:0, eye_color:0, height_scale:128 }
}
fn sample(character: &CharacterState, velocity: Vec3<f32>, physics: &PhysicsState,
          inventory: Option<&comp::Inventory>, ticks: u32) -> [anim::FigureBoneData; 16] {
    let mut state = world_animation::State::default();
    let mut bones = [anim::FigureBoneData::default();16];
    for tick in 0..ticks {
        let time = tick as f32 / 60.0;
        bones = state.update(body(), inventory, velocity * time, Quaternion::identity(), time,
            world_animation::Inputs { character, physics, velocity, scale:1.0, look_direction:Dir::default(),
                activity:None, volume_rider:None, riding:false, interaction_position:None,
                has_light:false, dt:1.0/60.0 });
    }
    assert!(bones.iter().flat_map(|bone| bone.0.iter().flatten()).all(|value|value.is_finite()));
    bones
}
fn difference(a: &[anim::FigureBoneData;16], b: &[anim::FigureBoneData;16]) -> f32 {
    a.iter().zip(b).flat_map(|(a,b)| a.0.iter().flatten().zip(b.0.iter().flatten()))
        .map(|(a,b)| (a-b).abs()).sum()
}
#[test] fn movement_and_airborne_poses_reach_the_original_bone_contract() {
    let idle = CharacterState::default();
    let grounded = PhysicsState { on_ground:Some(common::terrain::Block::empty()), ..Default::default() };
    let still = sample(&idle, Vec3::zero(), &grounded, None, 40);
    let running = sample(&idle, Vec3::unit_y()*8.0, &grounded, None, 40);
    assert!(difference(&still,&running)>0.1, "movement remained idle");
    let running_later = sample(&idle, Vec3::unit_y()*8.0, &grounded, None, 50);
    assert!(difference(&running,&running_later)>0.1, "run cycle did not advance");
    let jumping = sample(&idle, Vec3::new(0.0,3.0,2.0), &PhysicsState::default(), None, 40);
    assert!(difference(&running,&jumping)>0.1, "airborne pose remained running");
}
#[test] fn authored_weapon_combo_advances_with_server_action_timer() {
    use common::{comp::{inventory::loadout_builder::LoadoutBuilder, Item, ability::{SpecifiedAbility,Ability},
        item::{ToolKind, tool::ContextualIndex}, controller::InputKind}, states::{combo_melee2, utils::{AbilityInfo,HandInfo,StageSection}}};
    let inventory=comp::Inventory::with_loadout_humanoid(LoadoutBuilder::empty()
        .active_mainhand(Some(Item::new_from_asset_expect("common.items.weapons.sword.starter"))).build());
    let strike: combo_melee2::Strike<f32> = ron::from_str(r#"(
        melee_constructor:(kind:Slash(damage:1.0,poise:1.0,knockback:0.0,energy_regen:0.0),
            scaled:None,range:2.0,angle:1.0,multi_target:None,damage_effect:None,attack_effect:None),
        buildup_duration:0.4,swing_duration:0.4,hit_timing:0.5,recover_duration:0.4)"#).unwrap();
    // The manifest's sixth primary context is the server-resolved basic double slash.
    let info=AbilityInfo { tool:Some(ToolKind::Sword),hand:Some(HandInfo::TwoHanded),input:InputKind::Primary,
        input_attr:None,ability_meta:Default::default(),ability:Some(SpecifiedAbility { ability:Ability::ToolPrimary,context_index:Some(ContextualIndex(5)) }) };
    let mut attack=combo_melee2::Data { static_data:combo_melee2::StaticData {
        strikes:vec![strike.clone().to_duration(),strike.to_duration()],energy_cost_per_strike:0.0,
        specifier:None,auto_progress:false,ability_info:info }, exhausted:false,start_next_strike:false,
        timer:std::time::Duration::from_secs_f32(0.05),stage_section:StageSection::Action,completed_strikes:0,
        movement_modifier:None,ori_modifier:None };
    let grounded=PhysicsState { on_ground:Some(common::terrain::Block::empty()), ..Default::default() };
    let before=sample(&CharacterState::ComboMelee2(attack.clone()),Vec3::zero(),&grounded,Some(&inventory),40);
    attack.timer=std::time::Duration::from_secs_f32(0.3);
    let after=sample(&CharacterState::ComboMelee2(attack),Vec3::zero(),&grounded,Some(&inventory),40);
    assert!(difference(&before,&after)>0.1,"combat stage timer did not move the weapon pose");
    let idle=sample(&CharacterState::default(),Vec3::zero(),&grounded,Some(&inventory),40);
    assert!(difference(&after,&idle)>0.1,"attack remained idle");
}
