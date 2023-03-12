use itertools::izip;

use hotham::{glam::Vec3, hecs::World};

use crate::{
    xpbd_collisions::resolve_ecs_collisions,
    xpbd_shape_constraints::{
        damping_of_shape_matching_constraints, resolve_shape_matching_constraints, ShapeConstraint,
    },
    State,
};

pub struct SimulationParams {
    pub dt: f32,
    pub acc: Vec3,
    pub particle_mass: f32,
    pub shape_compliance: f32, // Inverse of physical stiffness
    pub shape_damping: f32, // Linear damping towards rigid body motion, fraction of speed per second
    pub stiction_factor: f32, // Maximum tangential correction per correction along normal.
}

pub fn xpbd_substep(
    world: &mut World,
    state: &mut State,
    &SimulationParams {
        dt,
        acc,
        particle_mass,
        shape_compliance,
        shape_damping,
        stiction_factor,
    }: &SimulationParams,
) {
    puffin::profile_function!();
    // Apply external forces
    {
        puffin::profile_scope!("Apply external forces");
        for vel in &mut state.velocities {
            *vel += acc * dt;
        }
    }

    // Predict new positions
    let mut points_next = {
        puffin::profile_scope!("Predict new positions");
        state
            .points_curr
            .iter()
            .zip(&state.velocities)
            .map(|(&curr, &vel)| curr + vel * dt)
            .collect::<Vec<_>>()
    };

    // TODO: Resolve distance constraints

    // Resolve shape matching constraints
    resolve_shape_matching_constraints(
        &mut points_next,
        &mut state.shape_constraints,
        shape_compliance,
        particle_mass.recip(),
        dt,
    );

    // Resolve collisions
    resolve_ecs_collisions(world, &mut points_next, stiction_factor);

    // Update velocities
    {
        puffin::profile_scope!("update_velocities");
        state.velocities = points_next
            .iter()
            .zip(&state.points_curr)
            .map(|(&next, &curr)| (next - curr) / dt)
            .collect::<Vec<_>>();
    }

    damping_of_shape_matching_constraints(
        &points_next,
        &mut state.velocities,
        &state.shape_constraints,
        shape_damping,
        dt,
    );

    state.points_curr = points_next;
}
