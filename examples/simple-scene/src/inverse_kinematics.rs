use std::collections::HashMap;

use enum_iterator::{all, cardinality, Sequence};
use serde::{Deserialize, Serialize};

use hotham::{
    asset_importer::add_model_to_world,
    components::{physics::SharedShape, Collider, LocalTransform, Stage},
    glam::{vec3, vec3a, Affine3A, Quat, Vec3A, Vec4},
    hecs::World,
    Engine,
};
use inline_tweak::tweak;

mod rr {
    pub use rerun::{
        components::{Box3D, ColorRGBA, Quaternion, Radius, Rigid3, Transform, Vec3D},
        time::{Time, TimeType, Timeline},
        MsgSender, Session,
    };
}

#[derive(Copy, Clone, Eq, Hash, Debug, PartialEq, Sequence, Deserialize, Serialize)]
#[repr(u8)]
pub enum IkNodeID {
    Hmd,
    HeadCenter,
    NeckRoot,
    Torso,
    Pelvis,
    Base,
    BalancePoint,
    LeftAim,
    LeftGrip,
    LeftPalm,
    LeftWrist,
    LeftLowerArm,
    LeftUpperArm,
    LeftUpperLeg,
    LeftLowerLeg,
    LeftFoot,
    RightAim,
    RightGrip,
    RightPalm,
    RightWrist,
    RightLowerArm,
    RightUpperArm,
    RightUpperLeg,
    RightLowerLeg,
    RightFoot,
}

pub struct IkNode {
    node_id: IkNodeID,
}

#[derive(Default)]
pub struct IkState {
    pub left_foot_in_stage: Option<Affine3A>,
    pub right_foot_in_stage: Option<Affine3A>,
    pub weight_distribution: WeightDistribution,
    pub node_positions: [Vec3A; cardinality::<IkNodeID>()],
    pub node_rotations: [Quat; cardinality::<IkNodeID>()],
}

#[derive(Clone, Copy)]
pub enum WeightDistribution {
    LeftPlanted,
    RightPlanted,
    SharedWeight,
}

impl Default for WeightDistribution {
    fn default() -> Self {
        Self::SharedWeight
    }
}

struct SphericalConstraint {
    node_a: IkNodeID,
    node_b: IkNodeID,
    point_in_a: Vec3A,
    point_in_b: Vec3A,
}

struct DistanceConstraint {
    node_a: IkNodeID,
    node_b: IkNodeID,
    point_in_a: Vec3A,
    point_in_b: Vec3A,
    distance: f32,
}

pub fn add_ik_nodes(models: &std::collections::HashMap<String, World>, world: &mut World) {
    let collider = Collider::new(SharedShape::ball(0.1));
    for node_id in all::<IkNodeID>() {
        let entity =
            add_model_to_world(model_name_from_node_id(node_id), models, world, None).unwrap();
        world
            .insert(entity, (collider.clone(), IkNode { node_id }))
            .unwrap();
    }
    let stages = world
        .query::<&Stage>()
        .iter()
        .map(|(entity, _)| entity)
        .collect::<Vec<_>>();
    for parent in stages {
        for node_id in all::<IkNodeID>() {
            let entity = add_model_to_world(
                model_name_from_node_id(node_id),
                models,
                world,
                Some(parent),
            )
            .unwrap();
            world
                .insert(entity, (collider.clone(), IkNode { node_id }))
                .unwrap();
        }
    }
}

fn model_name_from_node_id(node_id: IkNodeID) -> &'static str {
    match node_id {
        IkNodeID::Hmd => "Axes",
        IkNodeID::HeadCenter => "SmallAxes",
        IkNodeID::NeckRoot => "SmallAxes",
        IkNodeID::Base => "Axes",
        IkNodeID::BalancePoint => "SmallAxes",
        IkNodeID::LeftGrip | IkNodeID::RightGrip => "SmallAxes",
        IkNodeID::LeftAim | IkNodeID::RightAim => "TinyAxes",
        IkNodeID::LeftPalm | IkNodeID::RightPalm => "SmallAxes",
        IkNodeID::LeftWrist | IkNodeID::RightWrist => "CrossAxes",
        IkNodeID::LeftFoot | IkNodeID::RightFoot => "DiscXZ",
        _ => "SmallAxes",
    }
}

pub fn inverse_kinematics_system(
    engine: &mut Engine,
    state: &mut IkState,
    session: Option<&mut rr::Session>,
) {
    puffin::profile_function!();
    // Fixed transforms and parameters
    let head_center_in_hmd = Affine3A::from_translation(vec3(0.0, tweak!(0.0), tweak!(0.10)));
    let neck_root_in_head_center = Affine3A::from_translation(vec3(0.0, tweak!(-0.1), tweak!(0.0)));
    let left_wrist_in_palm =
        Affine3A::from_translation(vec3(tweak!(-0.015), tweak!(-0.01), tweak!(0.065)));
    let right_wrist_in_palm =
        Affine3A::from_translation((left_wrist_in_palm.translation * vec3a(-1.0, 1.0, 1.0)).into());
    let lower_arm_length = tweak!(0.28);
    let upper_arm_length = tweak!(0.28);
    let collarbone_length = tweak!(0.17);
    let shoulder_width = tweak!(0.40);
    let sternum_width = tweak!(0.06);
    let hip_width = tweak!(0.26);
    let sternum_height_in_torso = tweak!(0.20);
    let neck_root_height_in_torso = tweak!(0.22);
    let lower_back_height_in_torso = tweak!(-0.20);
    let lower_back_height_in_pelvis = tweak!(0.10);
    let hip_height_in_pelvis = tweak!(-0.07);
    let upper_leg_length = tweak!(0.40);
    let lower_leg_length = tweak!(0.40);
    let ankle_height = tweak!(0.10);
    let wrist_in_lower_arm = vec3a(0.0, 0.0, -lower_arm_length / 2.0);
    let elbow_in_lower_arm = vec3a(0.0, 0.0, lower_arm_length / 2.0);
    let elbow_in_upper_arm = vec3a(0.0, 0.0, -upper_arm_length / 2.0);
    let shoulder_in_upper_arm = vec3a(0.0, 0.0, upper_arm_length / 2.0);
    let left_shoulder_in_torso = vec3a(-shoulder_width / 2.0, sternum_height_in_torso, 0.0);
    let right_shoulder_in_torso = vec3a(shoulder_width / 2.0, sternum_height_in_torso, 0.0);
    let left_sc_joint_in_torso = vec3a(-sternum_width / 2.0, sternum_height_in_torso, 0.0);
    let right_sc_joint_in_torso = vec3a(sternum_width / 2.0, sternum_height_in_torso, 0.0);
    let neck_root_in_torso = vec3a(0.0, neck_root_height_in_torso, 0.0);
    let lower_back_in_torso = vec3a(0.0, lower_back_height_in_torso, 0.0);
    let lower_back_in_pelvis = vec3a(0.0, lower_back_height_in_pelvis, 0.0);
    let left_hip_in_pelvis = vec3a(-hip_width / 2.0, hip_height_in_pelvis, 0.0);
    let right_hip_in_pelvis = vec3a(hip_width / 2.0, hip_height_in_pelvis, 0.0);
    let hip_in_upper_leg = vec3a(0.0, upper_leg_length / 2.0, 0.0);
    let knee_in_upper_leg = vec3a(0.0, -upper_leg_length / 2.0, 0.0);
    let knee_in_lower_leg = vec3a(0.0, lower_leg_length / 2.0, 0.0);
    let ankle_in_lower_leg = vec3a(0.0, -lower_leg_length / 2.0, 0.0);
    let ankle_in_foot = vec3a(0.0, ankle_height, 0.0);
    let foot_radius = tweak!(0.1);
    let step_multiplier = tweak!(3.0);
    let step_size = foot_radius * (step_multiplier + 1.0);
    let stagger_threshold = foot_radius * tweak!(2.0);

    let spherical_constraints = [
        SphericalConstraint {
            // Left wrist
            node_a: IkNodeID::LeftPalm,
            node_b: IkNodeID::LeftLowerArm,
            point_in_a: left_wrist_in_palm.translation,
            point_in_b: wrist_in_lower_arm,
        },
        SphericalConstraint {
            // Right wrist
            node_a: IkNodeID::RightPalm,
            node_b: IkNodeID::RightLowerArm,
            point_in_a: right_wrist_in_palm.translation,
            point_in_b: wrist_in_lower_arm,
        },
        SphericalConstraint {
            // Left elbow
            node_a: IkNodeID::LeftLowerArm,
            node_b: IkNodeID::LeftUpperArm,
            point_in_a: elbow_in_lower_arm,
            point_in_b: elbow_in_upper_arm,
        },
        SphericalConstraint {
            // Right elbow
            node_a: IkNodeID::RightLowerArm,
            node_b: IkNodeID::RightUpperArm,
            point_in_a: elbow_in_lower_arm,
            point_in_b: elbow_in_upper_arm,
        },
        SphericalConstraint {
            // Neck
            node_a: IkNodeID::HeadCenter,
            node_b: IkNodeID::Torso,
            point_in_a: neck_root_in_head_center.translation,
            point_in_b: neck_root_in_torso,
        },
        SphericalConstraint {
            // Lower back
            node_a: IkNodeID::Torso,
            node_b: IkNodeID::Pelvis,
            point_in_a: lower_back_in_torso,
            point_in_b: lower_back_in_pelvis,
        },
        SphericalConstraint {
            // Left hip joint
            node_a: IkNodeID::Pelvis,
            node_b: IkNodeID::LeftUpperLeg,
            point_in_a: left_hip_in_pelvis,
            point_in_b: hip_in_upper_leg,
        },
        SphericalConstraint {
            // Right hip joint
            node_a: IkNodeID::Pelvis,
            node_b: IkNodeID::RightUpperLeg,
            point_in_a: right_hip_in_pelvis,
            point_in_b: hip_in_upper_leg,
        },
        SphericalConstraint {
            // Left knee
            node_a: IkNodeID::LeftUpperLeg,
            node_b: IkNodeID::LeftLowerLeg,
            point_in_a: knee_in_upper_leg,
            point_in_b: knee_in_lower_leg,
        },
        SphericalConstraint {
            // Right knee
            node_a: IkNodeID::RightUpperLeg,
            node_b: IkNodeID::RightLowerLeg,
            point_in_a: knee_in_upper_leg,
            point_in_b: knee_in_lower_leg,
        },
        SphericalConstraint {
            // Left ankle
            node_a: IkNodeID::LeftLowerLeg,
            node_b: IkNodeID::LeftFoot,
            point_in_a: ankle_in_lower_leg,
            point_in_b: ankle_in_foot,
        },
        SphericalConstraint {
            // Right ankle
            node_a: IkNodeID::RightLowerLeg,
            node_b: IkNodeID::RightFoot,
            point_in_a: ankle_in_lower_leg,
            point_in_b: ankle_in_foot,
        },
    ];
    let distance_constraints = [
        DistanceConstraint {
            // Left collarbone
            node_a: IkNodeID::LeftUpperArm,
            node_b: IkNodeID::Torso,
            point_in_a: shoulder_in_upper_arm,
            point_in_b: left_sc_joint_in_torso,
            distance: collarbone_length,
        },
        DistanceConstraint {
            // Right collarbone
            node_a: IkNodeID::RightUpperArm,
            node_b: IkNodeID::Torso,
            point_in_a: shoulder_in_upper_arm,
            point_in_b: right_sc_joint_in_torso,
            distance: collarbone_length,
        },
    ];

    // Dynamic transforms
    let world = &mut engine.world;
    let input_context = &engine.input_context;
    let hmd_in_stage = input_context.hmd.hmd_in_stage();
    let left_grip_in_stage = input_context.left.stage_from_grip();
    let left_aim_in_stage = input_context.left.stage_from_aim();
    let right_grip_in_stage = input_context.right.stage_from_grip();
    let right_aim_in_stage = input_context.right.stage_from_aim();
    let head_center_in_stage = hmd_in_stage * head_center_in_hmd;
    let neck_root_in_stage = head_center_in_stage * neck_root_in_head_center;
    let base_in_stage = {
        let mut base_in_stage = neck_root_in_stage;
        base_in_stage.translation.y = 0.0;
        let x_dir_in_stage = vec3a(
            base_in_stage.matrix3.x_axis.x,
            0.0,
            base_in_stage.matrix3.x_axis.z,
        )
        .normalize();
        base_in_stage.matrix3.x_axis = x_dir_in_stage;
        base_in_stage.matrix3.y_axis = Vec3A::Y;
        base_in_stage.matrix3.z_axis = x_dir_in_stage.cross(Vec3A::Y);
        base_in_stage
    };

    let left_palm_in_stage = {
        let mut left_palm_in_stage = left_grip_in_stage;
        left_palm_in_stage.matrix3 = left_aim_in_stage.matrix3;
        left_palm_in_stage
    };
    let right_palm_in_stage = {
        let mut right_palm_in_stage = right_grip_in_stage;
        right_palm_in_stage.matrix3 = right_aim_in_stage.matrix3;
        right_palm_in_stage
    };
    let left_wrist_in_stage = left_palm_in_stage * left_wrist_in_palm;
    let right_wrist_in_stage = right_palm_in_stage * right_wrist_in_palm;

    let left_foot_in_stage = state
        .left_foot_in_stage
        .unwrap_or_else(|| base_in_stage * Affine3A::from_translation(vec3(-0.2, 0.0, 0.0)));
    let right_foot_in_stage = state
        .right_foot_in_stage
        .unwrap_or_else(|| base_in_stage * Affine3A::from_translation(vec3(0.2, 0.0, 0.0)));
    let base_from_stage = base_in_stage.inverse();
    let left_foot_in_base = base_from_stage * left_foot_in_stage;
    let right_foot_in_base = base_from_stage * right_foot_in_stage;
    state.weight_distribution = match (
        left_foot_in_base.translation.length() < foot_radius,
        right_foot_in_base.translation.length() < foot_radius,
    ) {
        (true, true) => state.weight_distribution,
        (true, false) => WeightDistribution::LeftPlanted,
        (false, true) => WeightDistribution::RightPlanted,
        (false, false) => WeightDistribution::SharedWeight,
    };
    let balance_point_in_base = {
        let a = left_foot_in_base.translation;
        let b = right_foot_in_base.translation;
        let c = Vec3A::ZERO;
        let v = b - a;
        let t = (c - a).dot(v) / v.dot(v);
        a + v * t.clamp(0.0, 1.0)
    };
    match state.weight_distribution {
        WeightDistribution::RightPlanted => {
            state.left_foot_in_stage = Some(
                base_in_stage
                    * Affine3A::from_translation(vec3(
                        -step_multiplier * right_foot_in_base.translation.x,
                        -step_multiplier * right_foot_in_base.translation.y,
                        -step_multiplier * right_foot_in_base.translation.z,
                    )),
            );
            state.right_foot_in_stage = Some(right_foot_in_stage);
        }
        WeightDistribution::LeftPlanted => {
            state.left_foot_in_stage = Some(left_foot_in_stage);
            state.right_foot_in_stage = Some(
                base_in_stage
                    * Affine3A::from_translation(vec3(
                        -step_multiplier * left_foot_in_base.translation.x,
                        -step_multiplier * left_foot_in_base.translation.y,
                        -step_multiplier * left_foot_in_base.translation.z,
                    )),
            );
        }
        WeightDistribution::SharedWeight => {
            if balance_point_in_base.length() > stagger_threshold {
                // Stagger step, lift the foot that is loaded the least.
                let v1 = balance_point_in_base - left_foot_in_base.translation;
                let v2 = balance_point_in_base - right_foot_in_base.translation;
                if v1.length_squared() < v2.length_squared() {
                    let dir = -left_foot_in_base.translation.normalize();
                    state.left_foot_in_stage = Some(left_foot_in_stage);
                    state.right_foot_in_stage = Some(
                        base_in_stage
                            * Affine3A::from_translation(
                                (left_foot_in_base.translation + dir * step_size).into(),
                            ),
                    );
                } else {
                    let dir = -right_foot_in_base.translation.normalize();
                    state.left_foot_in_stage = Some(
                        base_in_stage
                            * Affine3A::from_translation(
                                (right_foot_in_base.translation + dir * step_size).into(),
                            ),
                    );
                    state.right_foot_in_stage = Some(right_foot_in_stage);
                }
            } else {
                state.left_foot_in_stage = Some(left_foot_in_stage);
                state.right_foot_in_stage = Some(right_foot_in_stage);
            }
        }
    }

    // Solve IK
    let fixed_nodes: [(IkNodeID, (Vec3A, Quat)); 15] = [
        (IkNodeID::Hmd, to_pos_rot(&hmd_in_stage)),
        (IkNodeID::HeadCenter, to_pos_rot(&head_center_in_stage)),
        (IkNodeID::NeckRoot, to_pos_rot(&neck_root_in_stage)),
        (IkNodeID::Base, to_pos_rot(&base_in_stage)),
        (
            IkNodeID::BalancePoint,
            to_pos_rot(&(base_in_stage * Affine3A::from_translation(balance_point_in_base.into()))),
        ),
        (IkNodeID::LeftGrip, to_pos_rot(&left_grip_in_stage)),
        (IkNodeID::LeftAim, to_pos_rot(&left_aim_in_stage)),
        (IkNodeID::LeftPalm, to_pos_rot(&left_palm_in_stage)),
        (IkNodeID::LeftWrist, to_pos_rot(&left_wrist_in_stage)),
        (IkNodeID::RightGrip, to_pos_rot(&right_grip_in_stage)),
        (IkNodeID::RightAim, to_pos_rot(&right_aim_in_stage)),
        (IkNodeID::RightPalm, to_pos_rot(&right_palm_in_stage)),
        (IkNodeID::RightWrist, to_pos_rot(&right_wrist_in_stage)),
        (IkNodeID::LeftFoot, to_pos_rot(&left_foot_in_stage)),
        (IkNodeID::RightFoot, to_pos_rot(&right_foot_in_stage)),
    ];
    for _ in 0..tweak!(10) {
        for (node_id, (pos, rot)) in fixed_nodes.iter() {
            state.node_positions[*node_id as usize] = *pos;
            state.node_rotations[*node_id as usize] = *rot;
        }
        for constraint in &spherical_constraints {
            let node_a = constraint.node_a as usize;
            let node_b = constraint.node_b as usize;
            let r1 = state.node_rotations[node_a] * constraint.point_in_a;
            let r2 = state.node_rotations[node_b] * constraint.point_in_b;
            // w = inv_mass + p.cross(n)ᵀ * inv_inertia * p.cross(n)
            let r1_squares = r1 * r1;
            let w1 = vec3a(
                1.0 + r1_squares.y + r1_squares.z,
                1.0 + r1_squares.z + r1_squares.x,
                1.0 + r1_squares.x + r1_squares.y,
            );
            let r2_squares = r2 * r2;
            let w2 = vec3a(
                1.0 + r2_squares.y + r2_squares.z,
                1.0 + r2_squares.z + r2_squares.x,
                1.0 + r2_squares.x + r2_squares.y,
            );
            let p1 = state.node_positions[node_a] + r1;
            let p2 = state.node_positions[node_b] + r2;
            let c = p1 - p2;
            let correction = -c / (w1 + w2);
            state.node_positions[node_a] += correction;
            state.node_positions[node_b] -= correction;
            // q1 <- q1 + 0.5 * (p1.cross(correction) * q1)
            let q1 = &mut state.node_rotations[node_a];
            let omega = r1.cross(correction);
            *q1 = Quat::from_vec4(
                Vec4::from(*q1) + 0.5 * Vec4::from(Quat::from_vec4(omega.extend(0.0)) * *q1),
            )
            .normalize();
            // q2 <- q2 - 0.5 * (p1.cross(correction) * q2)
            let q2 = &mut state.node_rotations[node_b];
            let omega = r2.cross(correction);
            *q2 = Quat::from_vec4(
                Vec4::from(*q2) - 0.5 * Vec4::from(Quat::from_vec4(omega.extend(0.0)) * *q2),
            )
            .normalize();
        }
        for constraint in &distance_constraints {
            let node_a = constraint.node_a as usize;
            let node_b = constraint.node_b as usize;
            let r1 = state.node_rotations[node_a] * constraint.point_in_a;
            let r2 = state.node_rotations[node_b] * constraint.point_in_b;
            // w = inv_mass + p.cross(n)ᵀ * inv_inertia * p.cross(n)
            let r1_squares = r1 * r1;
            let w1 = vec3a(
                1.0 + r1_squares.y + r1_squares.z,
                1.0 + r1_squares.z + r1_squares.x,
                1.0 + r1_squares.x + r1_squares.y,
            );
            let r2_squares = r2 * r2;
            let w2 = vec3a(
                1.0 + r2_squares.y + r2_squares.z,
                1.0 + r2_squares.z + r2_squares.x,
                1.0 + r2_squares.x + r2_squares.y,
            );
            let p1 = state.node_positions[node_a] + r1;
            let p2 = state.node_positions[node_b] + r2;
            let v = p1 - p2;
            let v_length = v.length();
            let c = v_length - constraint.distance;
            let correction = (-c / ((w1 + w2) * v_length)) * v;
            state.node_positions[node_a] += correction;
            state.node_positions[node_b] -= correction;
            // q1 <- q1 + 0.5 * (p1.cross(correction) * q1)
            let q1 = &mut state.node_rotations[node_a];
            let omega = r1.cross(correction);
            *q1 = Quat::from_vec4(
                Vec4::from(*q1) + 0.5 * Vec4::from(Quat::from_vec4(omega.extend(0.0)) * *q1),
            )
            .normalize();
            // q2 <- q2 - 0.5 * (p1.cross(correction) * q2)
            let q2 = &mut state.node_rotations[node_b];
            let omega = r2.cross(correction);
            *q2 = Quat::from_vec4(
                Vec4::from(*q2) - 0.5 * Vec4::from(Quat::from_vec4(omega.extend(0.0)) * *q2),
            )
            .normalize();
        }
    }

    // Update entity transforms
    for (_, (local_transform, node)) in world
        .query_mut::<(&mut LocalTransform, &IkNode)>()
        .into_iter()
    {
        let node_id = node.node_id as usize;
        local_transform.translation = state.node_positions[node_id].into();
        local_transform.rotation = state.node_rotations[node_id];
    }

    // Store snapshot of current state if menu button is pressed
    if input_context.left.menu_button_just_pressed() {
        let mut summary = HashMap::<IkNodeID, (Vec3A, Quat)>::new();
        for node_id in all::<IkNodeID>() {
            summary.insert(
                node_id,
                (
                    state.node_positions[node_id as usize],
                    state.node_rotations[node_id as usize],
                ),
            );
        }
        let serialized = serde_json::to_string(&summary).unwrap();
        let date_time = chrono::Local::now().naive_local();
        let filename = date_time
            .format("inverse_kinematics_snapshot_%Y-%m-%d_%H.%M.%S.json")
            .to_string();
        println!("[INVERSE_KINEMATICS] Storing snapshot to '{}'", filename);
        std::fs::write(&filename, serialized).expect(&format!("failed to write to '{filename}'"));
    }

    // Send poses to rerun
    if let Some(session) = session {
        let radius = rr::Radius(0.001);
        let log_fn = || -> hotham::anyhow::Result<()> {
            for node_id in all::<IkNodeID>() {
                let translation = &state.node_positions[node_id as usize];
                let rotation = &state.node_rotations[node_id as usize];
                let box_shape = match node_id {
                    IkNodeID::HeadCenter => rr::Box3D::new(0.08, 0.11, 0.11),
                    IkNodeID::Hmd => rr::Box3D::new(0.08, 0.04, 0.05),
                    IkNodeID::LeftAim
                    | IkNodeID::LeftGrip
                    | IkNodeID::LeftWrist
                    | IkNodeID::RightAim
                    | IkNodeID::RightGrip
                    | IkNodeID::RightWrist
                    | IkNodeID::BalancePoint
                    | IkNodeID::NeckRoot => rr::Box3D::new(0.01, 0.01, 0.01),
                    IkNodeID::Torso => {
                        rr::Box3D::new(shoulder_width / 2.0, sternum_height_in_torso, 0.10)
                    }
                    IkNodeID::Pelvis => rr::Box3D::new(hip_width / 2.0, hip_height_in_pelvis, 0.10),
                    IkNodeID::LeftFoot | IkNodeID::RightFoot | IkNodeID::Base => {
                        rr::Box3D::new(0.05, 0.001, 0.05)
                    }
                    IkNodeID::LeftPalm | IkNodeID::RightPalm => rr::Box3D::new(0.025, 0.05, 0.10),
                    IkNodeID::LeftLowerArm
                    | IkNodeID::LeftUpperArm
                    | IkNodeID::RightLowerArm
                    | IkNodeID::RightUpperArm => rr::Box3D::new(0.05, 0.05, 0.14),
                    IkNodeID::LeftUpperLeg
                    | IkNodeID::LeftLowerLeg
                    | IkNodeID::RightUpperLeg
                    | IkNodeID::RightLowerLeg => rr::Box3D::new(0.075, 0.075, 0.20),
                };
                rr::MsgSender::new(format!("stage/{:?}", node_id))
                    .with_component(&[rr::Transform::Rigid3(rr::Rigid3 {
                        rotation: rr::Quaternion {
                            w: rotation.w,
                            x: rotation.x,
                            y: rotation.y,
                            z: rotation.z,
                        },
                        translation: rr::Vec3D([translation.x, translation.y, translation.z]),
                    })])?
                    .with_splat(box_shape)?
                    .with_splat(radius)?
                    .send(session)?;
            }
            Ok(())
        };
        log_fn().unwrap_or_else(|e| {
            eprintln!("Failed to send poses to rerun: {e}");
        });
    }
}

fn to_pos_rot(transform: &Affine3A) -> (Vec3A, Quat) {
    let (_scale, rotation, translation) = transform.to_scale_rotation_translation();
    (translation.into(), rotation)
}
