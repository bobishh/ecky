use std::{
    collections::{BTreeMap, BTreeSet},
    time::Instant,
};

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    ElementAssembler, FaerSparseCholeskySolver, FemDirichletConstraint, FemIndexedTet4Mesh,
    FemMaterial, FemPoint3, FemSparseEntry, FemSparseMatrix, FemValidationError, LinearSolver,
    Tet4Element, Tet4Orientation, FEM_SCHEMA_VERSION,
};

mod mma;
mod reconstruction;
use mma::{conservative_lift_update, mma87_update, relative_kkt_residual, Mma87History};
pub use reconstruction::{
    extract_density_support_component, extract_density_support_graph,
    fit_symmetric_density_centerlines, reconstruct_density_surface, FemDensityAnchor,
    FemDensityCenterlineBranch, FemDensityCenterlineControls, FemDensityCenterlinePoint,
    FemDensitySupportComponent, FemDensitySupportGraph, FemDensitySupportGraphEdge,
    FemDensitySupportGraphNode, FemDensitySurfaceControls, FemDensitySurfaceMesh,
    FemSymmetricDensityCenterlines,
};

const MAXIMUM_GCMMA_INNER_ATTEMPTS: usize = 32;
const GCMMA_OBJECTIVE_SOLVER_TOLERANCE_FACTOR: f64 = 10.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GcmmaAttemptTrace {
    pub outer_iteration: usize,
    pub inner_attempt: usize,
    pub exact_objective: f64,
    pub approximate_objective: f64,
    pub exact_constraint: f64,
    pub approximate_constraint: f64,
    pub objective_gap: f64,
    pub constraint_gap: f64,
    pub objective_lift: f64,
    pub constraint_lift: f64,
    pub dual: f64,
    pub maximum_density_change: f64,
}

pub fn format_gcmma_attempt_trace(trace: &[GcmmaAttemptTrace]) -> String {
    let records = trace
        .iter()
        .map(|entry| {
            format!(
                "{{:outer {} :inner {} :exact-objective {:.6e} :approx-objective {:.6e} :exact-constraint {:.6e} :approx-constraint {:.6e} :objective-gap {:.6e} :constraint-gap {:.6e} :objective-lift {:.6e} :constraint-lift {:.6e} :dual {:.6e} :max-density-change {:.6e}}}",
                entry.outer_iteration,
                entry.inner_attempt,
                entry.exact_objective,
                entry.approximate_objective,
                entry.exact_constraint,
                entry.approximate_constraint,
                entry.objective_gap,
                entry.constraint_gap,
                entry.objective_lift,
                entry.constraint_lift,
                entry.dual,
                entry.maximum_density_change,
            )
        })
        .collect::<Vec<_>>();
    format!("[{}]", records.join(" "))
}
const REQUIRED_CONSECUTIVE_CONVERGED_ITERATIONS: usize = 3;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FemTopologyLoadCase {
    pub id: String,
    pub weight: f64,
    pub rhs_n: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FemTopologyControls {
    pub volume_fraction: f64,
    pub penalty: f64,
    pub minimum_density: f64,
    pub filter_radius_mm: f64,
    pub move_limit: f64,
    pub convergence_tolerance: f64,
    pub relative_solver_tolerance: f64,
    pub require_parallel_solver: bool,
    pub maximum_iterations: usize,
    pub maximum_dimension: usize,
    pub maximum_elements: usize,
    pub maximum_solve_count: usize,
    pub maximum_working_memory_bytes: usize,
    pub maximum_result_bytes: usize,
    pub maximum_wall_time_ms: u64,
    pub runtime_identity_digest: String,
    pub passive_solid_cells: Vec<usize>,
    pub passive_void_cells: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FemTopologyIteration {
    pub iteration: usize,
    pub compliance: f64,
    pub volume_fraction: f64,
    pub maximum_density_change: f64,
    pub maximum_physical_density_change: f64,
    pub kkt_residual: f64,
    pub conservative_inner_attempts: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FemMma87State {
    pub previous_design_densities: Vec<f64>,
    pub previous_previous_design_densities: Vec<f64>,
    pub asymptote_widths: Vec<f64>,
    pub dual: f64,
    pub objective_lift: f64,
    pub constraint_lift: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FemTopologyTermination {
    Paused,
    Cancelled,
    Converged,
    MaximumIterations,
    MaximumWallTime,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FemTopologyState {
    pub schema_version: u32,
    pub input_digest: String,
    pub design_densities: Vec<f64>,
    pub mma87: FemMma87State,
    pub initial_compliance: Option<f64>,
    pub solver_identity: Option<crate::FemLinearSolverIdentity>,
    pub iterations: Vec<FemTopologyIteration>,
    pub state_digest: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FemTopologyResult {
    pub schema_version: u32,
    pub initial_compliance: f64,
    pub final_compliance: f64,
    pub final_volume_fraction: f64,
    pub filter_radius_mm: f64,
    pub passive_solid_volume_fraction: f64,
    pub passive_void_volume_fraction: f64,
    pub densities: Vec<f64>,
    pub compliance_sensitivity: Vec<f64>,
    pub iterations: Vec<FemTopologyIteration>,
    pub solver_identity: Option<crate::FemLinearSolverIdentity>,
    pub termination: FemTopologyTermination,
    pub exact_brep: bool,
    pub production_step: bool,
    pub engineering_accepted: bool,
    pub result_digest: String,
}

#[derive(Debug)]
struct ElementData {
    dofs: [usize; 12],
    stiffness: [[f64; 12]; 12],
    volume: f64,
    centroid: FemPoint3,
}

#[derive(Debug)]
struct SharedDirichletReduction {
    matrix: FemSparseMatrix,
    right_hand_sides: Vec<Vec<f64>>,
    free_dof_indices: Vec<usize>,
    constrained_dofs: Vec<FemDirichletConstraint>,
    original_dimension: usize,
}

impl SharedDirichletReduction {
    fn recover_full_solution(
        &self,
        reduced_solution: &[f64],
    ) -> Result<Vec<f64>, FemValidationError> {
        if reduced_solution.len() != self.free_dof_indices.len() {
            return error("reducedSolution", "length differs from free DOF count");
        }
        let mut solution = vec![0.0; self.original_dimension];
        for (value, dof_index) in reduced_solution.iter().zip(&self.free_dof_indices) {
            if !value.is_finite() {
                return error("reducedSolution.value", "must be finite");
            }
            solution[*dof_index] = *value;
        }
        for constraint in &self.constrained_dofs {
            solution[constraint.dof_index] = constraint.value_mm;
        }
        Ok(solution)
    }
}

pub fn optimize_simp(
    mesh: &FemIndexedTet4Mesh,
    material: &FemMaterial,
    load_cases: &[FemTopologyLoadCase],
    constraints: &[FemDirichletConstraint],
    controls: &FemTopologyControls,
) -> Result<FemTopologyResult, FemValidationError> {
    let mut state = initialize_simp_state(mesh, material, load_cases, constraints, controls)?;
    let termination = advance_simp_state(
        mesh,
        material,
        load_cases,
        constraints,
        controls,
        &mut state,
        controls.maximum_iterations,
        || false,
    )?;
    finalize_simp_state(
        mesh,
        material,
        load_cases,
        constraints,
        controls,
        &state,
        termination,
    )
}

pub fn initialize_simp_state(
    mesh: &FemIndexedTet4Mesh,
    material: &FemMaterial,
    load_cases: &[FemTopologyLoadCase],
    constraints: &[FemDirichletConstraint],
    controls: &FemTopologyControls,
) -> Result<FemTopologyState, FemValidationError> {
    validate_inputs(mesh, material, load_cases, constraints, controls)?;
    let input_digest = topology_input_digest(mesh, material, load_cases, constraints, controls);
    let mut design_densities = vec![controls.volume_fraction; mesh.cells.len()];
    let solid = controls
        .passive_solid_cells
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let void = controls
        .passive_void_cells
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    impose_passive(
        &mut design_densities,
        &solid,
        &void,
        controls.minimum_density,
    );
    let mma87 = FemMma87State {
        previous_design_densities: design_densities.clone(),
        previous_previous_design_densities: design_densities.clone(),
        asymptote_widths: vec![0.5 * (1.0 - controls.minimum_density); mesh.cells.len()],
        dual: 0.0,
        objective_lift: 0.0,
        constraint_lift: 0.0,
    };
    let mut state = FemTopologyState {
        schema_version: FEM_SCHEMA_VERSION,
        input_digest,
        design_densities,
        mma87,
        initial_compliance: None,
        solver_identity: None,
        iterations: Vec::new(),
        state_digest: String::new(),
    };
    refresh_state_digest(&mut state);
    Ok(state)
}

#[allow(clippy::too_many_arguments)]
pub fn advance_simp_state<F>(
    mesh: &FemIndexedTet4Mesh,
    material: &FemMaterial,
    load_cases: &[FemTopologyLoadCase],
    constraints: &[FemDirichletConstraint],
    controls: &FemTopologyControls,
    state: &mut FemTopologyState,
    maximum_new_iterations: usize,
    cancelled: F,
) -> Result<FemTopologyTermination, FemValidationError>
where
    F: FnMut() -> bool,
{
    advance_simp_state_traced(
        mesh,
        material,
        load_cases,
        constraints,
        controls,
        state,
        maximum_new_iterations,
        cancelled,
        |_| {},
    )
}

#[allow(clippy::too_many_arguments)]
pub fn advance_simp_state_traced<F, T>(
    mesh: &FemIndexedTet4Mesh,
    material: &FemMaterial,
    load_cases: &[FemTopologyLoadCase],
    constraints: &[FemDirichletConstraint],
    controls: &FemTopologyControls,
    state: &mut FemTopologyState,
    maximum_new_iterations: usize,
    cancelled: F,
    trace_attempt: T,
) -> Result<FemTopologyTermination, FemValidationError>
where
    F: FnMut() -> bool,
    T: FnMut(GcmmaAttemptTrace),
{
    advance_simp_state_traced_checkpointed(
        mesh,
        material,
        load_cases,
        constraints,
        controls,
        state,
        maximum_new_iterations,
        cancelled,
        trace_attempt,
        |_| {},
    )
}

#[allow(clippy::too_many_arguments)]
pub fn advance_simp_state_traced_checkpointed<F, T, C>(
    mesh: &FemIndexedTet4Mesh,
    material: &FemMaterial,
    load_cases: &[FemTopologyLoadCase],
    constraints: &[FemDirichletConstraint],
    controls: &FemTopologyControls,
    state: &mut FemTopologyState,
    maximum_new_iterations: usize,
    mut cancelled: F,
    mut trace_attempt: T,
    mut checkpoint: C,
) -> Result<FemTopologyTermination, FemValidationError>
where
    F: FnMut() -> bool,
    T: FnMut(GcmmaAttemptTrace),
    C: FnMut(&FemTopologyState),
{
    validate_inputs(mesh, material, load_cases, constraints, controls)?;
    validate_state(state, mesh, material, load_cases, constraints, controls)?;
    let admitted_new_iterations = maximum_new_iterations.min(
        controls
            .maximum_iterations
            .saturating_sub(state.iterations.len()),
    );
    validate_advance_solve_budget(
        load_cases.len(),
        admitted_new_iterations,
        controls.maximum_solve_count,
    )?;
    let started = Instant::now();
    let elements = element_data(mesh, material)?;
    let stiffness_plan = StiffnessAssemblyPlan::new(mesh.nodes.len() * 3, &elements);
    let volumes = elements
        .iter()
        .map(|element| element.volume)
        .collect::<Vec<_>>();
    let mut wall_limited = false;
    let filter = {
        let mut stopped = || {
            if cancelled() {
                true
            } else if started.elapsed().as_millis() >= u128::from(controls.maximum_wall_time_ms) {
                wall_limited = true;
                true
            } else {
                false
            }
        };
        DensityFilter::new_bounded(
            &elements,
            controls.filter_radius_mm,
            filter_memory_budget(mesh, load_cases, controls)?,
            &mut stopped,
        )?
    };
    let Some(filter) = filter else {
        refresh_state_digest(state);
        return Ok(if wall_limited {
            FemTopologyTermination::MaximumWallTime
        } else {
            FemTopologyTermination::Cancelled
        });
    };
    let solid = controls
        .passive_solid_cells
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let void = controls
        .passive_void_cells
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let mut minimum_design = vec![controls.minimum_density; elements.len()];
    impose_passive(&mut minimum_design, &solid, &void, controls.minimum_density);
    let minimum_physical = physical_density(
        &filter,
        &minimum_design,
        &solid,
        &void,
        controls.minimum_density,
    );
    if volume_fraction(&minimum_physical, &volumes) > controls.volume_fraction + 1.0e-12 {
        return error(
            "volumeFraction",
            "target is infeasible with passive-solid cells and minimum density",
        );
    }
    let (volume_gradient, fixed_physical_volume, total_volume) =
        affine_physical_volume(&filter, &volumes, &solid, &void, controls.minimum_density);
    let free_design_cells = (0..elements.len())
        .filter(|index| !solid.contains(index) && !void.contains(index))
        .collect::<Vec<_>>();
    if state.iterations.is_empty() {
        let interior_volume_fraction = (controls.volume_fraction - controls.move_limit.min(0.01))
            .max(controls.minimum_density);
        let free_density = feasible_uniform_free_density(
            &volume_gradient,
            &free_design_cells,
            fixed_physical_volume,
            total_volume,
            interior_volume_fraction,
            controls.minimum_density,
        )?;
        for index in &free_design_cells {
            state.design_densities[*index] = free_density;
        }
        impose_passive(
            &mut state.design_densities,
            &solid,
            &void,
            controls.minimum_density,
        );
        state.mma87.previous_design_densities = state.design_densities.clone();
        state.mma87.previous_previous_design_densities = state.design_densities.clone();
        refresh_state_digest(state);
    }
    let stop_iteration = state
        .iterations
        .len()
        .saturating_add(maximum_new_iterations)
        .min(controls.maximum_iterations);
    while state.iterations.len() < stop_iteration {
        if cancelled() {
            refresh_state_digest(state);
            return Ok(FemTopologyTermination::Cancelled);
        }
        if started.elapsed().as_millis() >= u128::from(controls.maximum_wall_time_ms) {
            refresh_state_digest(state);
            return Ok(FemTopologyTermination::MaximumWallTime);
        }
        let physical = physical_density(
            &filter,
            &state.design_densities,
            &solid,
            &void,
            controls.minimum_density,
        );
        wall_limited = false;
        let analysis = {
            let mut stop_between_solves = || {
                if cancelled() {
                    true
                } else if started.elapsed().as_millis() >= u128::from(controls.maximum_wall_time_ms)
                {
                    wall_limited = true;
                    true
                } else {
                    false
                }
            };
            analyze_bounded(
                mesh.nodes.len(),
                &elements,
                &stiffness_plan,
                material,
                load_cases,
                constraints,
                &physical,
                controls,
                &mut stop_between_solves,
            )?
        };
        let Some((compliance, physical_gradient, solver_identity)) = analysis else {
            refresh_state_digest(state);
            return Ok(if wall_limited {
                FemTopologyTermination::MaximumWallTime
            } else {
                FemTopologyTermination::Cancelled
            });
        };
        state.initial_compliance.get_or_insert(compliance);
        state.solver_identity = Some(solver_identity);
        let design_gradient = filter.transpose(&physical_gradient);
        let objective_scale = compliance.abs().max(1.0);
        let normalized_compliance = compliance / objective_scale;
        let current_volume_fraction = volume_fraction(&physical, &volumes);
        let free_current = free_design_cells
            .iter()
            .map(|index| state.design_densities[*index])
            .collect::<Vec<_>>();
        let free_objective_gradient = free_design_cells
            .iter()
            .map(|index| design_gradient[*index] / objective_scale)
            .collect::<Vec<_>>();
        let free_constraint_gradient = free_design_cells
            .iter()
            .map(|index| volume_gradient[*index] / total_volume)
            .collect::<Vec<_>>();
        let history = Mma87History {
            previous: free_design_cells
                .iter()
                .map(|index| state.mma87.previous_design_densities[*index])
                .collect(),
            previous_previous: free_design_cells
                .iter()
                .map(|index| state.mma87.previous_previous_design_densities[*index])
                .collect(),
            asymptote_widths: free_design_cells
                .iter()
                .map(|index| state.mma87.asymptote_widths[*index])
                .collect(),
            dual: state.mma87.dual,
        };
        let mut objective_lift = (state.mma87.objective_lift / 10.0).max(1.0e-5);
        let mut constraint_lift = (state.mma87.constraint_lift / 10.0).max(1.0e-5);
        let mut conservative_inner_attempts = 0;
        let mut conservative_trace = Vec::with_capacity(MAXIMUM_GCMMA_INNER_ATTEMPTS);
        let step = loop {
            conservative_inner_attempts += 1;
            let step = mma87_update(
                &free_current,
                normalized_compliance,
                &free_objective_gradient,
                current_volume_fraction - controls.volume_fraction,
                &free_constraint_gradient,
                objective_lift,
                constraint_lift,
                controls.minimum_density,
                1.0,
                controls.move_limit,
                state.iterations.len() + 1,
                &history,
            )
            .map_err(|message| FemValidationError {
                field: "topology.gcmma".into(),
                message: message.into(),
            })?;
            let mut candidate_design = state.design_densities.clone();
            for (index, density) in free_design_cells.iter().zip(&step.design) {
                candidate_design[*index] = *density;
            }
            let candidate_physical = physical_density(
                &filter,
                &candidate_design,
                &solid,
                &void,
                controls.minimum_density,
            );
            wall_limited = false;
            let candidate_analysis = {
                let mut stop_between_solves = || {
                    if cancelled() {
                        true
                    } else if started.elapsed().as_millis()
                        >= u128::from(controls.maximum_wall_time_ms)
                    {
                        wall_limited = true;
                        true
                    } else {
                        false
                    }
                };
                analyze_bounded(
                    mesh.nodes.len(),
                    &elements,
                    &stiffness_plan,
                    material,
                    load_cases,
                    constraints,
                    &candidate_physical,
                    controls,
                    &mut stop_between_solves,
                )?
            };
            let Some((candidate_compliance, _, _)) = candidate_analysis else {
                refresh_state_digest(state);
                return Ok(if wall_limited {
                    FemTopologyTermination::MaximumWallTime
                } else {
                    FemTopologyTermination::Cancelled
                });
            };
            let candidate_constraint =
                volume_fraction(&candidate_physical, &volumes) - controls.volume_fraction;
            let normalized_candidate_compliance = candidate_compliance / objective_scale;
            let normalized_objective_gap =
                normalized_candidate_compliance - step.approximate_objective;
            let objective_gap = normalized_objective_gap * objective_scale;
            let constraint_gap = candidate_constraint - step.approximate_constraint;
            let maximum_density_change = free_current
                .iter()
                .zip(&step.design)
                .map(|(current, candidate)| (candidate - current).abs())
                .fold(0.0_f64, f64::max);
            let trace_entry = GcmmaAttemptTrace {
                outer_iteration: state.iterations.len() + 1,
                inner_attempt: conservative_inner_attempts,
                exact_objective: candidate_compliance,
                approximate_objective: step.approximate_objective * objective_scale,
                exact_constraint: candidate_constraint,
                approximate_constraint: step.approximate_constraint,
                objective_gap,
                constraint_gap,
                objective_lift: objective_lift * objective_scale,
                constraint_lift,
                dual: step.history.dual,
                maximum_density_change,
            };
            conservative_trace.push(trace_entry);
            trace_attempt(trace_entry);
            if objective_is_conservative_with_solver_tolerance(
                normalized_candidate_compliance,
                step.approximate_objective,
                controls.relative_solver_tolerance,
            ) && constraint_gap <= 1.0e-12
            {
                break step;
            }
            if conservative_inner_attempts >= MAXIMUM_GCMMA_INNER_ATTEMPTS {
                return error(
                    "topology.gcmma",
                    &format!(
                        "conservative approximation did not upper-bound exact functions within the inner-attempt limit: trace-edn={}",
                        format_gcmma_attempt_trace(&conservative_trace)
                    ),
                );
            }
            let conservative_weight = free_current
                .iter()
                .zip(&step.design)
                .zip(&step.history.asymptote_widths)
                .map(|((current, candidate), width)| {
                    let change_squared = (candidate - current).powi(2);
                    change_squared / (width * width - change_squared).max(1.0e-15)
                })
                .sum::<f64>()
                / 2.0;
            if !conservative_weight.is_finite() || conservative_weight <= 0.0 {
                return error("topology.gcmma", "conservative lift weight is invalid");
            }
            objective_lift = conservative_lift_update(
                objective_lift,
                normalized_objective_gap,
                conservative_weight,
            );
            constraint_lift =
                conservative_lift_update(constraint_lift, constraint_gap, conservative_weight);
        };
        let kkt_residual = relative_kkt_residual(
            &free_current,
            &free_objective_gradient,
            &free_constraint_gradient,
            step.history.dual,
            current_volume_fraction - controls.volume_fraction,
            controls.minimum_density,
            1.0,
        );
        let mut next = state.design_densities.clone();
        for (index, density) in free_design_cells.iter().zip(&step.design) {
            next[*index] = *density;
        }
        let maximum_density_change = state
            .design_densities
            .iter()
            .zip(&next)
            .map(|(left, right)| (left - right).abs())
            .fold(0.0, f64::max);
        state.mma87.previous_previous_design_densities =
            state.mma87.previous_design_densities.clone();
        state.mma87.previous_design_densities = state.design_densities.clone();
        for (index, width) in free_design_cells.iter().zip(&step.history.asymptote_widths) {
            state.mma87.asymptote_widths[*index] = *width;
        }
        state.mma87.dual = step.history.dual;
        state.mma87.objective_lift = objective_lift;
        state.mma87.constraint_lift = constraint_lift;
        state.design_densities = next;
        let updated_physical = physical_density(
            &filter,
            &state.design_densities,
            &solid,
            &void,
            controls.minimum_density,
        );
        let maximum_physical_density_change = physical
            .iter()
            .zip(&updated_physical)
            .map(|(left, right)| (left - right).abs())
            .fold(0.0, f64::max);
        state.iterations.push(FemTopologyIteration {
            iteration: state.iterations.len() + 1,
            compliance,
            volume_fraction: volume_fraction(&updated_physical, &volumes),
            maximum_density_change,
            maximum_physical_density_change,
            kkt_residual,
            conservative_inner_attempts,
        });
        refresh_state_digest(state);
        checkpoint(state);
        let convergence_is_sustained = state.iterations.len()
            >= REQUIRED_CONSECUTIVE_CONVERGED_ITERATIONS
            && state
                .iterations
                .iter()
                .rev()
                .take(REQUIRED_CONSECUTIVE_CONVERGED_ITERATIONS)
                .all(|iteration| {
                    iteration.maximum_physical_density_change <= controls.convergence_tolerance
                        && iteration.kkt_residual <= controls.convergence_tolerance
                });
        if convergence_is_sustained {
            refresh_state_digest(state);
            return Ok(FemTopologyTermination::Converged);
        }
    }

    refresh_state_digest(state);
    Ok(if state.iterations.len() >= controls.maximum_iterations {
        FemTopologyTermination::MaximumIterations
    } else {
        FemTopologyTermination::Paused
    })
}

#[allow(clippy::too_many_arguments)]
pub fn finalize_simp_state(
    mesh: &FemIndexedTet4Mesh,
    material: &FemMaterial,
    load_cases: &[FemTopologyLoadCase],
    constraints: &[FemDirichletConstraint],
    controls: &FemTopologyControls,
    state: &FemTopologyState,
    termination: FemTopologyTermination,
) -> Result<FemTopologyResult, FemValidationError> {
    validate_inputs(mesh, material, load_cases, constraints, controls)?;
    validate_state(state, mesh, material, load_cases, constraints, controls)?;
    if matches!(
        termination,
        FemTopologyTermination::Cancelled | FemTopologyTermination::MaximumWallTime
    ) {
        return error(
            "termination",
            "cancelled or wall-limited state must publish a resumable checkpoint without final analysis",
        );
    }
    let elements = element_data(mesh, material)?;
    let volumes = elements
        .iter()
        .map(|element| element.volume)
        .collect::<Vec<_>>();
    let filter = DensityFilter::new(
        &elements,
        controls.filter_radius_mm,
        filter_memory_budget(mesh, load_cases, controls)?,
    )?;
    let solid = controls
        .passive_solid_cells
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let void = controls
        .passive_void_cells
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let densities = physical_density(
        &filter,
        &state.design_densities,
        &solid,
        &void,
        controls.minimum_density,
    );
    let (final_compliance, compliance_sensitivity, solver_identity) = analyze(
        mesh.nodes.len(),
        &elements,
        material,
        load_cases,
        constraints,
        &densities,
        controls,
    )?;
    let total_volume = volumes.iter().sum::<f64>();
    let mut result = FemTopologyResult {
        schema_version: FEM_SCHEMA_VERSION,
        initial_compliance: state.initial_compliance.unwrap_or(final_compliance),
        final_compliance,
        final_volume_fraction: volume_fraction(&densities, &volumes),
        filter_radius_mm: controls.filter_radius_mm,
        passive_solid_volume_fraction: solid.iter().map(|index| volumes[*index]).sum::<f64>()
            / total_volume,
        passive_void_volume_fraction: void.iter().map(|index| volumes[*index]).sum::<f64>()
            / total_volume,
        densities,
        compliance_sensitivity: filter.transpose(&compliance_sensitivity),
        iterations: state.iterations.clone(),
        solver_identity: Some(solver_identity),
        termination,
        exact_brep: false,
        production_step: false,
        engineering_accepted: false,
        result_digest: String::new(),
    };
    let encoded_bytes = canonical_result_bytes(&result);
    result.result_digest = sha256_bytes(&encoded_bytes);
    if encoded_bytes.len() > controls.maximum_result_bytes {
        return error(
            "maximumResultBytes",
            "topology result exceeds declared byte budget",
        );
    }
    Ok(result)
}

fn validate_inputs(
    mesh: &FemIndexedTet4Mesh,
    material: &FemMaterial,
    loads: &[FemTopologyLoadCase],
    constraints: &[FemDirichletConstraint],
    controls: &FemTopologyControls,
) -> Result<(), FemValidationError> {
    material.validate()?;
    if mesh.schema_version != FEM_SCHEMA_VERSION || mesh.nodes.is_empty() || mesh.cells.is_empty() {
        return error("mesh", "must be a non-empty schema-v1 Tet4 mesh");
    }
    if loads.is_empty() || constraints.is_empty() {
        return error("loadCases", "loads and constraints must not be empty");
    }
    let dimension = mesh.nodes.len() * 3;
    if mesh.cells.len() > controls.maximum_elements {
        return error(
            "maximumElements",
            "Tet4 design domain exceeds declared element budget",
        );
    }
    let mut load_ids = BTreeSet::new();
    for load in loads {
        if load.id.trim().is_empty()
            || !load.weight.is_finite()
            || load.weight <= 0.0
            || load.rhs_n.len() != dimension
            || load.rhs_n.iter().any(|value| !value.is_finite())
        {
            return error(
                "loadCases",
                "each load requires an id, positive weight, and finite full-size RHS",
            );
        }
        if !load_ids.insert(load.id.as_str()) {
            return error("loadCases.id", "load case ids must be unique");
        }
    }
    let valid_unit = |value: f64| value.is_finite() && value > 0.0 && value < 1.0;
    if !valid_unit(controls.volume_fraction)
        || !valid_unit(controls.minimum_density)
        || !controls.penalty.is_finite()
        || controls.penalty < 1.0
        || !controls.filter_radius_mm.is_finite()
        || controls.filter_radius_mm <= 0.0
        || !valid_unit(controls.move_limit)
        || !controls.convergence_tolerance.is_finite()
        || controls.convergence_tolerance <= 0.0
        || !controls.relative_solver_tolerance.is_finite()
        || controls.relative_solver_tolerance <= 0.0
        || controls.maximum_iterations == 0
        || controls.maximum_dimension < dimension
        || controls.maximum_elements == 0
        || controls.maximum_solve_count == 0
        || controls.maximum_working_memory_bytes == 0
        || controls.maximum_result_bytes == 0
        || controls.maximum_wall_time_ms == 0
        || controls.runtime_identity_digest.trim().is_empty()
    {
        return error("controls", "contains invalid or insufficient bounds");
    }
    let base_working_bytes = topology_base_working_bytes(mesh, loads)?;
    if base_working_bytes > controls.maximum_working_memory_bytes {
        return error(
            "maximumWorkingMemoryBytes",
            "topology assembly and solve workspace exceeds declared memory budget",
        );
    }
    let solid = controls
        .passive_solid_cells
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let void = controls
        .passive_void_cells
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    if solid.len() != controls.passive_solid_cells.len()
        || void.len() != controls.passive_void_cells.len()
        || !solid.is_disjoint(&void)
        || solid
            .iter()
            .chain(&void)
            .any(|cell| *cell >= mesh.cells.len())
    {
        return error("passiveCells", "must be unique, disjoint, and in range");
    }
    Ok(())
}

fn validate_advance_solve_budget(
    load_case_count: usize,
    maximum_new_iterations: usize,
    maximum_solve_count: usize,
) -> Result<(), FemValidationError> {
    let required_solves = topology_required_solve_capacity(maximum_new_iterations, load_case_count);
    if required_solves > maximum_solve_count {
        return error(
            "maximumSolveCount",
            "declared solve budget cannot cover requested bounded conservative inner attempts",
        );
    }
    Ok(())
}

/// Conservative runtime capacity: baseline plus every bounded GCMMA inner retry,
/// independently solved for every load case.
pub fn topology_required_solve_capacity(
    maximum_new_iterations: usize,
    load_case_count: usize,
) -> usize {
    maximum_new_iterations
        .saturating_mul(MAXIMUM_GCMMA_INNER_ATTEMPTS.saturating_add(1))
        .saturating_add(1)
        .saturating_mul(load_case_count)
}

fn objective_is_conservative_with_solver_tolerance(
    exact: f64,
    approximate: f64,
    relative_solver_tolerance: f64,
) -> bool {
    let scale = exact.abs().max(approximate.abs()).max(1.0);
    let relative_tolerance = (GCMMA_OBJECTIVE_SOLVER_TOLERANCE_FACTOR * relative_solver_tolerance)
        .max(64.0 * f64::EPSILON);
    exact - approximate <= relative_tolerance * scale
}

fn topology_base_working_bytes(
    mesh: &FemIndexedTet4Mesh,
    loads: &[FemTopologyLoadCase],
) -> Result<usize, FemValidationError> {
    let cells = mesh.cells.len();
    let dimension = mesh
        .nodes
        .len()
        .checked_mul(3)
        .ok_or_else(|| FemValidationError {
            field: "maximumWorkingMemoryBytes".into(),
            message: "topology dimension memory estimate overflowed".into(),
        })?;
    let element_bytes = cells
        .checked_mul(std::mem::size_of::<ElementData>())
        .ok_or_else(|| FemValidationError {
            field: "maximumWorkingMemoryBytes".into(),
            message: "element workspace estimate overflowed".into(),
        })?;
    let sparse_bytes = cells
        .checked_mul(12 * 12)
        .and_then(|entries| entries.checked_mul(64))
        .ok_or_else(|| FemValidationError {
            field: "maximumWorkingMemoryBytes".into(),
            message: "sparse assembly workspace estimate overflowed".into(),
        })?;
    let vector_count = loads.len().saturating_add(12);
    let vector_bytes = dimension
        .checked_mul(vector_count)
        .and_then(|values| values.checked_mul(std::mem::size_of::<f64>()))
        .ok_or_else(|| FemValidationError {
            field: "maximumWorkingMemoryBytes".into(),
            message: "solver vector workspace estimate overflowed".into(),
        })?;
    element_bytes
        .checked_add(sparse_bytes)
        .and_then(|bytes| bytes.checked_add(vector_bytes))
        .ok_or_else(|| FemValidationError {
            field: "maximumWorkingMemoryBytes".into(),
            message: "topology base workspace estimate overflowed".into(),
        })
}

fn filter_memory_budget(
    mesh: &FemIndexedTet4Mesh,
    loads: &[FemTopologyLoadCase],
    controls: &FemTopologyControls,
) -> Result<usize, FemValidationError> {
    let base = topology_base_working_bytes(mesh, loads)?;
    controls
        .maximum_working_memory_bytes
        .checked_sub(base)
        .ok_or_else(|| FemValidationError {
            field: "maximumWorkingMemoryBytes".into(),
            message: "no memory remains for density filter".into(),
        })
}

fn topology_input_digest(
    mesh: &FemIndexedTet4Mesh,
    material: &FemMaterial,
    load_cases: &[FemTopologyLoadCase],
    constraints: &[FemDirichletConstraint],
    controls: &FemTopologyControls,
) -> String {
    let mut load_cases = load_cases.iter().collect::<Vec<_>>();
    load_cases.sort_by(|left, right| left.id.cmp(&right.id));
    let mut constraints = constraints.iter().collect::<Vec<_>>();
    constraints.sort_by(|left, right| {
        left.dof_index
            .cmp(&right.dof_index)
            .then_with(|| left.value_mm.total_cmp(&right.value_mm))
    });
    let mut solid = controls.passive_solid_cells.clone();
    let mut void = controls.passive_void_cells.clone();
    solid.sort_unstable();
    void.sort_unstable();
    let mut bytes = CanonicalBytes::new(b"ECKY-TOPOLOGY-INPUT-V3");
    bytes.u32(mesh.schema_version);
    bytes.points(&mesh.nodes);
    bytes.u64(mesh.cells.len() as u64);
    for cell in &mesh.cells {
        for node in cell {
            bytes.u32(*node);
        }
    }
    bytes.u32(material.schema_version);
    bytes.string(&material.name);
    bytes.f64(material.young_modulus_mpa);
    bytes.f64(material.poisson_ratio);
    bytes.f64(material.density_kg_per_mm3);
    bytes.f64(material.yield_strength_mpa);
    bytes.u64(load_cases.len() as u64);
    for load in load_cases {
        bytes.string(&load.id);
        bytes.f64(load.weight);
        bytes.f64s(&load.rhs_n);
    }
    bytes.u64(constraints.len() as u64);
    for constraint in constraints {
        bytes.u64(constraint.dof_index as u64);
        bytes.f64(constraint.value_mm);
    }
    bytes.f64(controls.volume_fraction);
    bytes.f64(controls.penalty);
    bytes.f64(controls.minimum_density);
    bytes.f64(controls.filter_radius_mm);
    bytes.f64(controls.move_limit);
    bytes.f64(controls.convergence_tolerance);
    bytes.f64(controls.relative_solver_tolerance);
    bytes.string(&controls.runtime_identity_digest);
    bytes.usizes(&solid);
    bytes.usizes(&void);
    sha256_bytes(&bytes.finish())
}

fn refresh_state_digest(state: &mut FemTopologyState) {
    state.state_digest.clear();
    state.state_digest = topology_state_digest(state);
}

pub fn topology_state_digest(state: &FemTopologyState) -> String {
    let mut bytes = CanonicalBytes::new(b"ECKY-TOPOLOGY-STATE-V2");
    bytes.u32(state.schema_version);
    bytes.string(&state.input_digest);
    bytes.f64s(&state.design_densities);
    bytes.f64s(&state.mma87.previous_design_densities);
    bytes.f64s(&state.mma87.previous_previous_design_densities);
    bytes.f64s(&state.mma87.asymptote_widths);
    bytes.f64(state.mma87.dual);
    bytes.f64(state.mma87.objective_lift);
    bytes.f64(state.mma87.constraint_lift);
    match state.initial_compliance {
        Some(value) => {
            bytes.u8(1);
            bytes.f64(value);
        }
        None => bytes.u8(0),
    }
    match &state.solver_identity {
        Some(value) => {
            bytes.u8(1);
            bytes.solver_identity(value);
        }
        None => bytes.u8(0),
    }
    bytes.iterations(&state.iterations);
    sha256_bytes(&bytes.finish())
}

pub fn topology_result_digest(result: &FemTopologyResult) -> String {
    sha256_bytes(&canonical_result_bytes(result))
}

fn canonical_result_bytes(result: &FemTopologyResult) -> Vec<u8> {
    let mut bytes = CanonicalBytes::new(b"ECKY-TOPOLOGY-RESULT-V2");
    bytes.u32(result.schema_version);
    bytes.f64(result.initial_compliance);
    bytes.f64(result.final_compliance);
    bytes.f64(result.final_volume_fraction);
    bytes.f64(result.filter_radius_mm);
    bytes.f64(result.passive_solid_volume_fraction);
    bytes.f64(result.passive_void_volume_fraction);
    bytes.f64s(&result.densities);
    bytes.f64s(&result.compliance_sensitivity);
    bytes.iterations(&result.iterations);
    match &result.solver_identity {
        Some(value) => {
            bytes.u8(1);
            bytes.solver_identity(value);
        }
        None => bytes.u8(0),
    }
    bytes.u8(termination_code(result.termination));
    bytes.u8(result.exact_brep as u8);
    bytes.u8(result.production_step as u8);
    bytes.u8(result.engineering_accepted as u8);
    bytes.finish()
}

fn termination_code(value: FemTopologyTermination) -> u8 {
    match value {
        FemTopologyTermination::Paused => 0,
        FemTopologyTermination::Cancelled => 1,
        FemTopologyTermination::Converged => 2,
        FemTopologyTermination::MaximumIterations => 3,
        FemTopologyTermination::MaximumWallTime => 4,
    }
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{:x}", hasher.finalize())
}

struct CanonicalBytes(Vec<u8>);

impl CanonicalBytes {
    fn new(magic: &[u8]) -> Self {
        Self(magic.to_vec())
    }

    fn finish(self) -> Vec<u8> {
        self.0
    }

    fn u8(&mut self, value: u8) {
        self.0.push(value);
    }

    fn u32(&mut self, value: u32) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    fn f64(&mut self, value: f64) {
        let canonical = if value == 0.0 { 0.0 } else { value };
        self.0.extend_from_slice(&canonical.to_bits().to_le_bytes());
    }

    fn string(&mut self, value: &str) {
        self.u64(value.len() as u64);
        self.0.extend_from_slice(value.as_bytes());
    }

    fn f64s(&mut self, values: &[f64]) {
        self.u64(values.len() as u64);
        for value in values {
            self.f64(*value);
        }
    }

    fn usizes(&mut self, values: &[usize]) {
        self.u64(values.len() as u64);
        for value in values {
            self.u64(*value as u64);
        }
    }

    fn points(&mut self, values: &[FemPoint3]) {
        self.u64(values.len() as u64);
        for value in values {
            self.f64(value.x_mm);
            self.f64(value.y_mm);
            self.f64(value.z_mm);
        }
    }

    fn iterations(&mut self, values: &[FemTopologyIteration]) {
        self.u64(values.len() as u64);
        for value in values {
            self.u64(value.iteration as u64);
            self.f64(value.compliance);
            self.f64(value.volume_fraction);
            self.f64(value.maximum_density_change);
            self.f64(value.maximum_physical_density_change);
            self.f64(value.kkt_residual);
            self.u64(value.conservative_inner_attempts as u64);
        }
    }

    fn solver_identity(&mut self, value: &crate::FemLinearSolverIdentity) {
        self.string(&value.backend);
        self.string(&value.backend_version);
        self.string(&value.factorization);
        self.string(&value.ordering);
        self.string(&value.scalar_type);
        self.string(&value.parallelism);
        self.u64(value.thread_count as u64);
        self.f64(value.relative_tolerance);
    }
}

fn validate_state(
    state: &FemTopologyState,
    mesh: &FemIndexedTet4Mesh,
    material: &FemMaterial,
    load_cases: &[FemTopologyLoadCase],
    constraints: &[FemDirichletConstraint],
    controls: &FemTopologyControls,
) -> Result<(), FemValidationError> {
    if state.schema_version != FEM_SCHEMA_VERSION
        || state.input_digest
            != topology_input_digest(mesh, material, load_cases, constraints, controls)
        || state.design_densities.len() != mesh.cells.len()
        || state.mma87.previous_design_densities.len() != mesh.cells.len()
        || state.mma87.previous_previous_design_densities.len() != mesh.cells.len()
        || state.mma87.asymptote_widths.len() != mesh.cells.len()
        || state.design_densities.iter().any(|density| {
            !density.is_finite() || *density < controls.minimum_density || *density > 1.0
        })
        || state
            .mma87
            .previous_design_densities
            .iter()
            .chain(&state.mma87.previous_previous_design_densities)
            .any(|density| {
                !density.is_finite() || *density < controls.minimum_density || *density > 1.0
            })
        || state
            .mma87
            .asymptote_widths
            .iter()
            .any(|width| !width.is_finite() || *width <= 0.0)
        || !state.mma87.dual.is_finite()
        || state.mma87.dual < 0.0
        || !state.mma87.objective_lift.is_finite()
        || state.mma87.objective_lift < 0.0
        || !state.mma87.constraint_lift.is_finite()
        || state.mma87.constraint_lift < 0.0
    {
        return error(
            "topologyState",
            "state does not match admitted optimization input",
        );
    }
    let trace_is_valid = state.iterations.len() <= controls.maximum_iterations
        && state
            .iterations
            .iter()
            .enumerate()
            .all(|(index, iteration)| {
                iteration.iteration == index + 1
                    && iteration.compliance.is_finite()
                    && iteration.compliance > 0.0
                    && iteration.volume_fraction.is_finite()
                    && iteration.volume_fraction > 0.0
                    && iteration.volume_fraction <= 1.0
                    && iteration.maximum_density_change.is_finite()
                    && iteration.maximum_density_change >= 0.0
                    && iteration.maximum_density_change <= 1.0
                    && iteration.maximum_physical_density_change.is_finite()
                    && iteration.maximum_physical_density_change >= 0.0
                    && iteration.maximum_physical_density_change <= 1.0
                    && iteration.kkt_residual.is_finite()
                    && iteration.kkt_residual >= 0.0
                    && (1..=MAXIMUM_GCMMA_INNER_ATTEMPTS)
                        .contains(&iteration.conservative_inner_attempts)
            });
    let initial_is_valid = match (state.initial_compliance, state.iterations.first()) {
        (None, None) => true,
        (Some(initial), Some(first)) => {
            initial.is_finite() && initial > 0.0 && initial.to_bits() == first.compliance.to_bits()
        }
        _ => false,
    };
    let solver_is_valid = (state.iterations.is_empty() && state.solver_identity.is_none())
        || (!state.iterations.is_empty() && state.solver_identity.is_some());
    if !trace_is_valid || !initial_is_valid || !solver_is_valid {
        return error(
            "topologyState.iterations",
            "iteration trace must be finite, sequential, bounded, and consistent with initial compliance",
        );
    }
    for index in &controls.passive_solid_cells {
        if state.design_densities[*index].to_bits() != 1.0_f64.to_bits() {
            return error(
                "topologyState.designDensities",
                "passive-solid design density changed",
            );
        }
    }
    for index in &controls.passive_void_cells {
        if state.design_densities[*index].to_bits() != controls.minimum_density.to_bits() {
            return error(
                "topologyState.designDensities",
                "passive-void design density changed",
            );
        }
    }
    let mut canonical = state.clone();
    refresh_state_digest(&mut canonical);
    if canonical.state_digest != state.state_digest {
        return error("topologyState.stateDigest", "state digest mismatch");
    }
    Ok(())
}

fn element_data(
    mesh: &FemIndexedTet4Mesh,
    material: &FemMaterial,
) -> Result<Vec<ElementData>, FemValidationError> {
    let assembler = ElementAssembler;
    mesh.cells
        .iter()
        .enumerate()
        .map(|(cell_index, cell)| {
            let indices = cell.map(|value| value as usize);
            if indices.iter().any(|index| *index >= mesh.nodes.len()) {
                return error(
                    &format!("mesh.cells[{cell_index}]"),
                    "contains out-of-range node",
                );
            }
            let element = Tet4Element::new(indices.map(|index| mesh.nodes[index]));
            if assembler.orientation(&element)? != Tet4Orientation::Positive {
                return error(
                    &format!("mesh.cells[{cell_index}]"),
                    "must have positive orientation",
                );
            }
            let volume = assembler.signed_volume_mm3(&element)?;
            let centroid = FemPoint3::new(
                element.nodes.iter().map(|point| point.x_mm).sum::<f64>() / 4.0,
                element.nodes.iter().map(|point| point.y_mm).sum::<f64>() / 4.0,
                element.nodes.iter().map(|point| point.z_mm).sum::<f64>() / 4.0,
            );
            Ok(ElementData {
                dofs: std::array::from_fn(|local| indices[local / 3] * 3 + local % 3),
                stiffness: assembler.stiffness_matrix(&element, material)?,
                volume,
                centroid,
            })
        })
        .collect()
}

fn analyze(
    node_count: usize,
    elements: &[ElementData],
    material: &FemMaterial,
    loads: &[FemTopologyLoadCase],
    constraints: &[FemDirichletConstraint],
    densities: &[f64],
    controls: &FemTopologyControls,
) -> Result<(f64, Vec<f64>, crate::FemLinearSolverIdentity), FemValidationError> {
    let stiffness_plan = StiffnessAssemblyPlan::new(node_count * 3, elements);
    analyze_bounded(
        node_count,
        elements,
        &stiffness_plan,
        material,
        loads,
        constraints,
        densities,
        controls,
        &mut || false,
    )?
    .ok_or_else(|| FemValidationError {
        field: "topology.analysis".into(),
        message: "unbounded analysis stopped unexpectedly".into(),
    })
}

#[allow(clippy::too_many_arguments)]
fn analyze_bounded<F>(
    node_count: usize,
    elements: &[ElementData],
    stiffness_plan: &StiffnessAssemblyPlan,
    material: &FemMaterial,
    loads: &[FemTopologyLoadCase],
    constraints: &[FemDirichletConstraint],
    densities: &[f64],
    controls: &FemTopologyControls,
    cancelled: &mut F,
) -> Result<Option<(f64, Vec<f64>, crate::FemLinearSolverIdentity)>, FemValidationError>
where
    F: FnMut() -> bool,
{
    debug_assert_eq!(stiffness_plan.dimension, node_count * 3);
    let ratios = densities
        .iter()
        .map(|rho| simp_scale(*rho, controls.minimum_density, controls.penalty))
        .collect::<Vec<_>>();
    let stiffness = stiffness_plan.assemble(elements, &ratios);
    let mut ordered = loads.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| left.id.cmp(&right.id));
    let Some(reduced) = reduce_shared_dirichlet(
        &stiffness,
        &ordered
            .iter()
            .map(|load| load.rhs_n.as_slice())
            .collect::<Vec<_>>(),
        constraints,
        cancelled,
    )?
    else {
        return Ok(None);
    };
    let mut compliance = 0.0;
    let mut solutions = Vec::with_capacity(ordered.len());
    let solved_systems =
        solve_topology_systems(&reduced.matrix, &reduced.right_hand_sides, controls)?;
    let solver_identity = solved_systems
        .first()
        .map(|solved| solved.solver_identity.clone())
        .ok_or_else(|| FemValidationError {
            field: "topology.analysis".into(),
            message: "solver returned no load cases".into(),
        })?;
    for (load, solved) in ordered.into_iter().zip(solved_systems) {
        let displacement = reduced.recover_full_solution(&solved.solution)?;
        compliance += load.weight
            * load
                .rhs_n
                .iter()
                .zip(&displacement)
                .map(|(force, movement)| force * movement)
                .sum::<f64>();
        solutions.push((load.weight, displacement));
    }
    let gradient = compliance_sensitivity(
        elements,
        &solutions,
        densities,
        controls.minimum_density,
        controls.penalty,
    );
    if !compliance.is_finite() || gradient.iter().any(|value| !value.is_finite()) {
        return error(
            "topology.analysis",
            "produced non-finite compliance or sensitivity",
        );
    }
    let _ = material;
    Ok(Some((compliance, gradient, solver_identity)))
}

fn compliance_sensitivity(
    elements: &[ElementData],
    solutions: &[(f64, Vec<f64>)],
    densities: &[f64],
    minimum_density: f64,
    penalty: f64,
) -> Vec<f64> {
    elements
        .par_iter()
        .enumerate()
        .map(|(index, element)| {
            let derivative = simp_scale_derivative(densities[index], minimum_density, penalty);
            solutions
                .iter()
                .fold(0.0, |gradient, (weight, displacement)| {
                    let u = element.dofs.map(|dof| displacement[dof]);
                    gradient - weight * derivative * quadratic(&element.stiffness, &u)
                })
        })
        .collect()
}

fn reduce_shared_dirichlet<F>(
    matrix: &FemSparseMatrix,
    right_hand_sides: &[&[f64]],
    constraints: &[FemDirichletConstraint],
    stopped: &mut F,
) -> Result<Option<SharedDirichletReduction>, FemValidationError>
where
    F: FnMut() -> bool,
{
    if matrix.dimension == 0 || right_hand_sides.is_empty() {
        return error(
            "matrix",
            "must be non-empty with at least one right-hand side",
        );
    }
    let mut constrained = BTreeMap::new();
    for constraint in constraints {
        if constraint.dof_index >= matrix.dimension {
            return error("constraints.dofIndex", "is out of range");
        }
        if !constraint.value_mm.is_finite() {
            return error("constraints.valueMm", "must be finite");
        }
        if constrained
            .insert(constraint.dof_index, constraint.value_mm)
            .is_some()
        {
            return error("constraints.dofIndex", "contains duplicate constrained DOF");
        }
    }
    let free_dof_indices = (0..matrix.dimension)
        .filter(|index| !constrained.contains_key(index))
        .collect::<Vec<_>>();
    if free_dof_indices.is_empty() {
        return error(
            "constraints",
            "constrain every DOF; reduced system is empty",
        );
    }
    let mut reduced_index = vec![usize::MAX; matrix.dimension];
    for (reduced, original) in free_dof_indices.iter().copied().enumerate() {
        reduced_index[original] = reduced;
    }
    let mut reduced_rhs = Vec::with_capacity(right_hand_sides.len());
    for rhs in right_hand_sides {
        if rhs.len() != matrix.dimension || rhs.iter().any(|value| !value.is_finite()) {
            return error("rhs", "must be finite and match matrix dimension");
        }
        reduced_rhs.push(
            free_dof_indices
                .iter()
                .map(|index| rhs[*index])
                .collect::<Vec<_>>(),
        );
    }

    let mut entries = Vec::with_capacity(matrix.entries.len());
    let mut previous = None;
    for (entry_index, entry) in matrix.entries.iter().enumerate() {
        if entry_index % 16_384 == 0 && stopped() {
            return Ok(None);
        }
        if entry.row >= matrix.dimension || entry.col >= matrix.dimension {
            return error("matrix.entries", "contains out-of-range index");
        }
        if !entry.value.is_finite() {
            return error("matrix.entries.value", "must be finite");
        }
        let coordinate = (entry.row, entry.col);
        if previous.is_some_and(|prior| prior >= coordinate) {
            return error(
                "matrix.entries",
                "must be unique and in canonical row-major order",
            );
        }
        previous = Some(coordinate);
        let reduced_row = reduced_index[entry.row];
        if reduced_row == usize::MAX {
            continue;
        }
        if let Some(constrained_value) = constrained.get(&entry.col) {
            for rhs in &mut reduced_rhs {
                rhs[reduced_row] -= entry.value * constrained_value;
            }
            continue;
        }
        let reduced_col = reduced_index[entry.col];
        entries.push(FemSparseEntry {
            row: reduced_row,
            col: reduced_col,
            value: entry.value,
        });
    }
    Ok(Some(SharedDirichletReduction {
        matrix: FemSparseMatrix {
            dimension: free_dof_indices.len(),
            entries,
        },
        right_hand_sides: reduced_rhs,
        free_dof_indices,
        constrained_dofs: constrained
            .into_iter()
            .map(|(dof_index, value_mm)| FemDirichletConstraint {
                dof_index,
                value_mm,
            })
            .collect(),
        original_dimension: matrix.dimension,
    }))
}

fn solve_topology_systems(
    matrix: &FemSparseMatrix,
    right_hand_sides: &[Vec<f64>],
    controls: &FemTopologyControls,
) -> Result<Vec<crate::FemLinearSolveResult>, FemValidationError> {
    if controls.require_parallel_solver {
        #[cfg(target_os = "macos")]
        {
            return crate::AccelerateSparseCholeskySolver.solve_many(
                matrix,
                right_hand_sides,
                controls.relative_solver_tolerance,
                controls.maximum_dimension,
            );
        }
        #[cfg(not(target_os = "macos"))]
        {
            return error(
                "solver.parallelism",
                "parallel topology solver is unavailable on this platform",
            );
        }
    }
    FaerSparseCholeskySolver.solve_many(
        matrix,
        right_hand_sides,
        controls.relative_solver_tolerance,
        controls.maximum_dimension,
    )
}

fn simp_scale(density: f64, minimum_stiffness_ratio: f64, penalty: f64) -> f64 {
    minimum_stiffness_ratio + (1.0 - minimum_stiffness_ratio) * density.max(1.0e-12).powf(penalty)
}

fn simp_scale_derivative(density: f64, minimum_stiffness_ratio: f64, penalty: f64) -> f64 {
    penalty * (1.0 - minimum_stiffness_ratio) * density.max(1.0e-12).powf(penalty - 1.0)
}

#[derive(Debug)]
struct StiffnessAssemblyPlan {
    dimension: usize,
    coordinates: Vec<(usize, usize)>,
    element_slots: Vec<[usize; 144]>,
}

impl StiffnessAssemblyPlan {
    fn new(dimension: usize, elements: &[ElementData]) -> Self {
        debug_assert_eq!(dimension % 3, 0);
        let node_count = dimension / 3;
        let mut node_neighbors = vec![Vec::<usize>::new(); node_count];
        for element in elements {
            let nodes = [
                element.dofs[0] / 3,
                element.dofs[3] / 3,
                element.dofs[6] / 3,
                element.dofs[9] / 3,
            ];
            for row_node in nodes {
                node_neighbors[row_node].extend_from_slice(&nodes);
            }
        }
        for neighbors in &mut node_neighbors {
            neighbors.sort_unstable();
            neighbors.dedup();
        }

        let mut row_offsets = Vec::with_capacity(dimension + 1);
        let mut coordinates = Vec::<(usize, usize)>::new();
        row_offsets.push(0);
        for row in 0..dimension {
            for column_node in &node_neighbors[row / 3] {
                for component in 0..3 {
                    coordinates.push((row, column_node * 3 + component));
                }
            }
            row_offsets.push(coordinates.len());
        }
        let mut element_slots = Vec::with_capacity(elements.len());
        for element in elements {
            let nodes = [
                element.dofs[0] / 3,
                element.dofs[3] / 3,
                element.dofs[6] / 3,
                element.dofs[9] / 3,
            ];
            let mut neighbor_positions = [[0usize; 4]; 4];
            for (local_row_node, row_node) in nodes.iter().copied().enumerate() {
                for (local_col_node, col_node) in nodes.iter().copied().enumerate() {
                    neighbor_positions[local_row_node][local_col_node] = node_neighbors[row_node]
                        .binary_search(&col_node)
                        .expect("element node must exist in its adjacency row");
                }
            }
            let mut slots = [0usize; 144];
            for row in 0..12 {
                for col in 0..12 {
                    let global_row = element.dofs[row];
                    let neighbor_position = neighbor_positions[row / 3][col / 3];
                    slots[row * 12 + col] =
                        row_offsets[global_row] + neighbor_position * 3 + col % 3;
                }
            }
            element_slots.push(slots);
        }
        Self {
            dimension,
            coordinates,
            element_slots,
        }
    }

    fn assemble(&self, elements: &[ElementData], scales: &[f64]) -> FemSparseMatrix {
        debug_assert_eq!(elements.len(), scales.len());
        debug_assert_eq!(elements.len(), self.element_slots.len());
        let mut values = vec![0.0; self.coordinates.len()];
        for ((element, scale), slots) in elements.iter().zip(scales).zip(&self.element_slots) {
            for row in 0..12 {
                for col in 0..12 {
                    let value = scale * element.stiffness[row][col];
                    if value != 0.0 {
                        values[slots[row * 12 + col]] += value;
                    }
                }
            }
        }
        FemSparseMatrix {
            dimension: self.dimension,
            entries: self
                .coordinates
                .iter()
                .copied()
                .zip(values)
                .filter_map(|((row, col), value)| {
                    (value != 0.0).then_some(FemSparseEntry { row, col, value })
                })
                .collect(),
        }
    }
}

#[cfg(test)]
fn assemble_scaled(dimension: usize, elements: &[ElementData], scales: &[f64]) -> FemSparseMatrix {
    StiffnessAssemblyPlan::new(dimension, elements).assemble(elements, scales)
}

#[cfg(test)]
fn assemble_scaled_reference(
    dimension: usize,
    elements: &[ElementData],
    scales: &[f64],
) -> FemSparseMatrix {
    let mut entries = BTreeMap::<(usize, usize), f64>::new();
    for (element, scale) in elements.iter().zip(scales) {
        for row in 0..12 {
            for col in 0..12 {
                *entries
                    .entry((element.dofs[row], element.dofs[col]))
                    .or_default() += scale * element.stiffness[row][col];
            }
        }
    }
    FemSparseMatrix {
        dimension,
        entries: entries
            .into_iter()
            .filter_map(|((row, col), value)| {
                (value != 0.0).then_some(FemSparseEntry { row, col, value })
            })
            .collect(),
    }
}

fn quadratic(matrix: &[[f64; 12]; 12], vector: &[f64; 12]) -> f64 {
    (0..12)
        .map(|row| {
            (0..12)
                .map(|col| vector[row] * matrix[row][col] * vector[col])
                .sum::<f64>()
        })
        .sum()
}

#[derive(Debug)]
struct DensityFilter {
    rows: Vec<Vec<(usize, f64)>>,
    columns: Vec<Vec<(usize, f64)>>,
}

impl DensityFilter {
    fn new(
        elements: &[ElementData],
        radius: f64,
        maximum_bytes: usize,
    ) -> Result<Self, FemValidationError> {
        Self::new_bounded(elements, radius, maximum_bytes, || false)?.ok_or_else(|| {
            FemValidationError {
                field: "filter".into(),
                message: "unbounded density filter construction stopped unexpectedly".into(),
            }
        })
    }

    fn new_bounded<F>(
        elements: &[ElementData],
        radius: f64,
        maximum_bytes: usize,
        mut stopped: F,
    ) -> Result<Option<Self>, FemValidationError>
    where
        F: FnMut() -> bool,
    {
        let mut rows = Vec::with_capacity(elements.len());
        let row_bytes = std::mem::size_of::<Vec<(usize, f64)>>()
            .checked_mul(elements.len())
            .and_then(|bytes| bytes.checked_mul(2))
            .ok_or_else(|| FemValidationError {
                field: "maximumWorkingMemoryBytes".into(),
                message: "filter row memory estimate overflowed".into(),
            })?;
        if row_bytes > maximum_bytes {
            return error(
                "maximumWorkingMemoryBytes",
                "density filter rows exceed declared memory budget",
            );
        }
        // Conservative bound: one BTreeMap node per element, including key,
        // links, Vec header, allocation metadata, and one stored index.
        let spatial_index_bytes =
            128usize
                .checked_mul(elements.len())
                .ok_or_else(|| FemValidationError {
                    field: "maximumWorkingMemoryBytes".into(),
                    message: "filter spatial-index memory estimate overflowed".into(),
                })?;
        let mut used_bytes =
            row_bytes
                .checked_add(spatial_index_bytes)
                .ok_or_else(|| FemValidationError {
                    field: "maximumWorkingMemoryBytes".into(),
                    message: "filter memory estimate overflowed".into(),
                })?;
        if used_bytes > maximum_bytes {
            return error(
                "maximumWorkingMemoryBytes",
                "density filter spatial index exceeds declared memory budget",
            );
        }

        let cell_of = |point: FemPoint3| {
            (
                (point.x_mm / radius).floor() as i64,
                (point.y_mm / radius).floor() as i64,
                (point.z_mm / radius).floor() as i64,
            )
        };
        let mut spatial_index = BTreeMap::<(i64, i64, i64), Vec<usize>>::new();
        for (index, element) in elements.iter().enumerate() {
            if stopped() {
                return Ok(None);
            }
            spatial_index
                .entry(cell_of(element.centroid))
                .or_default()
                .push(index);
        }

        for target in elements {
            if stopped() {
                return Ok(None);
            }
            let mut row = Vec::new();
            let (cell_x, cell_y, cell_z) = cell_of(target.centroid);
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        if let Some(indices) =
                            spatial_index.get(&(cell_x + dx, cell_y + dy, cell_z + dz))
                        {
                            for index in indices {
                                let source = &elements[*index];
                                let dx = target.centroid.x_mm - source.centroid.x_mm;
                                let dy = target.centroid.y_mm - source.centroid.y_mm;
                                let dz = target.centroid.z_mm - source.centroid.z_mm;
                                let distance = (dx * dx + dy * dy + dz * dz).sqrt();
                                let weight = (radius - distance).max(0.0) * source.volume;
                                if weight <= 0.0 {
                                    continue;
                                }
                                used_bytes = used_bytes
                                    .checked_add(2 * std::mem::size_of::<(usize, f64)>())
                                    .ok_or_else(|| FemValidationError {
                                        field: "maximumWorkingMemoryBytes".into(),
                                        message: "filter memory estimate overflowed".into(),
                                    })?;
                                if used_bytes > maximum_bytes {
                                    return error(
                                        "maximumWorkingMemoryBytes",
                                        "density filter neighbors exceed declared memory budget",
                                    );
                                }
                                row.push((*index, weight));
                            }
                        }
                    }
                }
            }
            let sum = row.iter().map(|(_, weight)| weight).sum::<f64>();
            if !sum.is_finite() || sum <= 0.0 {
                return error("filterRadiusMm", "produced an empty filter row");
            }
            for (_, weight) in &mut row {
                *weight /= sum;
            }
            rows.push(row);
        }
        let mut columns = vec![Vec::new(); rows.len()];
        for (row_index, row) in rows.iter().enumerate() {
            for (column_index, weight) in row {
                columns[*column_index].push((row_index, *weight));
            }
        }
        Ok(Some(Self { rows, columns }))
    }

    fn forward(&self, values: &[f64]) -> Vec<f64> {
        self.rows
            .par_iter()
            .map(|row| {
                row.iter()
                    .map(|(index, weight)| weight * values[*index])
                    .sum()
            })
            .collect()
    }

    fn transpose(&self, values: &[f64]) -> Vec<f64> {
        self.columns
            .par_iter()
            .map(|column| {
                column
                    .iter()
                    .map(|(row_index, weight)| weight * values[*row_index])
                    .sum()
            })
            .collect()
    }
}

#[allow(clippy::too_many_arguments)]
#[allow(dead_code)]
#[cfg(test)]
fn oc_update(
    current: &[f64],
    compliance_gradient: &[f64],
    volume_gradient: &[f64],
    fixed_physical_volume: f64,
    total_volume: f64,
    free_design_cells: &[usize],
    controls: &FemTopologyControls,
) -> Result<Vec<f64>, FemValidationError> {
    let mut low: f64 = 0.0;
    let mut high: f64 = 1.0e12;
    let mut candidate = current.to_vec();
    let mut is_free = vec![false; current.len()];
    for index in free_design_cells {
        is_free[*index] = true;
    }
    let fixed_design_volume = fixed_physical_volume
        + current
            .iter()
            .zip(volume_gradient)
            .enumerate()
            .filter(|(index, _)| !is_free[*index])
            .map(|(_, (density, gradient))| density * gradient)
            .sum::<f64>();
    let mut achieved = f64::NAN;
    for _ in 0..96 {
        let multiplier = 0.5 * (low + high);
        let mut physical_volume = fixed_design_volume;
        for index in free_design_cells.iter().copied() {
            let ratio = (-compliance_gradient[index]
                / (volume_gradient[index].max(1.0e-18) * multiplier.max(1.0e-18)))
            .max(0.0)
            .sqrt();
            candidate[index] = (current[index] * ratio)
                .clamp(
                    current[index] - controls.move_limit,
                    current[index] + controls.move_limit,
                )
                .clamp(controls.minimum_density, 1.0);
            physical_volume += candidate[index] * volume_gradient[index];
        }
        achieved = physical_volume / total_volume;
        if achieved > controls.volume_fraction {
            low = multiplier;
        } else {
            high = multiplier;
        }
    }
    if achieved > controls.volume_fraction + 1.0e-3 && high >= 0.999e12 {
        return error(
            "volumeFraction",
            "target is infeasible with passive-solid cells",
        );
    }
    Ok(candidate)
}

fn affine_physical_volume(
    filter: &DensityFilter,
    volumes: &[f64],
    solid: &BTreeSet<usize>,
    void: &BTreeSet<usize>,
    minimum_density: f64,
) -> (Vec<f64>, f64, f64) {
    let mut free_output_volumes = volumes.to_vec();
    for index in solid.iter().chain(void) {
        free_output_volumes[*index] = 0.0;
    }
    let fixed_physical_volume = solid.iter().map(|index| volumes[*index]).sum::<f64>()
        + void
            .iter()
            .map(|index| minimum_density * volumes[*index])
            .sum::<f64>();
    (
        filter.transpose(&free_output_volumes),
        fixed_physical_volume,
        volumes.iter().sum(),
    )
}

fn feasible_uniform_free_density(
    volume_gradient: &[f64],
    free_design_cells: &[usize],
    fixed_physical_volume: f64,
    total_volume: f64,
    target_volume_fraction: f64,
    minimum_density: f64,
) -> Result<f64, FemValidationError> {
    let free_weight = free_design_cells
        .iter()
        .map(|index| volume_gradient[*index])
        .sum::<f64>();
    if !free_weight.is_finite() || free_weight <= 0.0 {
        return error(
            "volumeFraction",
            "topology design domain contains no free physical volume",
        );
    }
    let density = ((target_volume_fraction * total_volume - fixed_physical_volume) / free_weight)
        .clamp(minimum_density, 1.0);
    let achieved = (fixed_physical_volume + density * free_weight) / total_volume;
    if achieved > target_volume_fraction + 1.0e-12 {
        return error(
            "volumeFraction",
            "target is infeasible with passive-solid cells and minimum density",
        );
    }
    Ok(density)
}

#[cfg(test)]
fn affine_volume_fraction(
    design: &[f64],
    volume_gradient: &[f64],
    fixed_physical_volume: f64,
    total_volume: f64,
) -> f64 {
    (design
        .iter()
        .zip(volume_gradient)
        .map(|(density, gradient)| density * gradient)
        .sum::<f64>()
        + fixed_physical_volume)
        / total_volume
}

fn physical_density(
    filter: &DensityFilter,
    design: &[f64],
    solid: &BTreeSet<usize>,
    void: &BTreeSet<usize>,
    minimum_density: f64,
) -> Vec<f64> {
    let mut physical = filter.forward(design);
    impose_passive(&mut physical, solid, void, minimum_density);
    physical
}

fn impose_passive(
    values: &mut [f64],
    solid: &BTreeSet<usize>,
    void: &BTreeSet<usize>,
    minimum_density: f64,
) {
    for index in solid {
        values[*index] = 1.0;
    }
    for index in void {
        values[*index] = minimum_density;
    }
}

fn volume_fraction(densities: &[f64], volumes: &[f64]) -> f64 {
    densities
        .iter()
        .zip(volumes)
        .map(|(rho, volume)| rho * volume)
        .sum::<f64>()
        / volumes.iter().sum::<f64>()
}

fn error<T>(field: &str, message: &str) -> Result<T, FemValidationError> {
    Err(FemValidationError {
        field: field.to_string(),
        message: message.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simp_derivative_matches_central_finite_difference() {
        let density = 0.47;
        let minimum = 1.0e-3;
        let penalty = 3.0;
        let epsilon = 1.0e-6;
        let finite_difference = (simp_scale(density + epsilon, minimum, penalty)
            - simp_scale(density - epsilon, minimum, penalty))
            / (2.0 * epsilon);
        let analytic = simp_scale_derivative(density, minimum, penalty);
        assert!((analytic - finite_difference).abs() <= 1.0e-9);
    }

    #[test]
    fn volume_aware_filter_preserves_constant_field_and_has_exact_adjoint() {
        let elements = [
            mock_element(FemPoint3::new(0.0, 0.0, 0.0), 1.0),
            mock_element(FemPoint3::new(0.5, 0.0, 0.0), 3.0),
            mock_element(FemPoint3::new(1.0, 0.0, 0.0), 7.0),
        ];
        let filter = DensityFilter::new(&elements, 1.1, usize::MAX).expect("filter");
        let constant = filter.forward(&[0.37; 3]);
        assert!(constant.iter().all(|value| (value - 0.37).abs() <= 1.0e-14));

        let x = [0.2, 0.5, 0.8];
        let y = [1.2, -0.7, 0.3];
        let hx = filter.forward(&x);
        let hty = filter.transpose(&y);
        let left = hx.iter().zip(y).map(|(a, b)| a * b).sum::<f64>();
        let right = x.iter().zip(hty).map(|(a, b)| a * b).sum::<f64>();
        assert!((left - right).abs() <= 1.0e-14);
    }

    #[test]
    fn spatial_filter_matches_canonical_all_pairs_reference() {
        let elements = [
            mock_element(FemPoint3::new(-1.1, 0.0, 0.0), 1.0),
            mock_element(FemPoint3::new(-0.1, 0.1, 0.0), 2.0),
            mock_element(FemPoint3::new(0.9, -0.1, 0.2), 3.0),
            mock_element(FemPoint3::new(1.8, 0.0, -0.2), 4.0),
            mock_element(FemPoint3::new(0.0, 1.7, 0.0), 5.0),
        ];
        let radius = 1.75;
        let indexed = DensityFilter::new(&elements, radius, usize::MAX).unwrap();
        let reference_rows = elements
            .iter()
            .map(|target| {
                let mut row = elements
                    .iter()
                    .enumerate()
                    .filter_map(|(index, source)| {
                        let dx = target.centroid.x_mm - source.centroid.x_mm;
                        let dy = target.centroid.y_mm - source.centroid.y_mm;
                        let dz = target.centroid.z_mm - source.centroid.z_mm;
                        let distance = (dx * dx + dy * dy + dz * dz).sqrt();
                        let weight = (radius - distance).max(0.0) * source.volume;
                        (weight > 0.0).then_some((index, weight))
                    })
                    .collect::<Vec<_>>();
                let sum = row.iter().map(|(_, weight)| weight).sum::<f64>();
                for (_, weight) in &mut row {
                    *weight /= sum;
                }
                row
            })
            .collect::<Vec<_>>();
        for (mut actual, expected) in indexed.rows.clone().into_iter().zip(reference_rows) {
            assert!(actual.windows(2).all(|pair| pair[0].0 < pair[1].0));
            actual.sort_unstable_by_key(|(index, _)| *index);
            assert_eq!(actual.len(), expected.len());
            for ((actual_index, actual_weight), (expected_index, expected_weight)) in
                actual.into_iter().zip(expected)
            {
                assert_eq!(actual_index, expected_index);
                assert!((actual_weight - expected_weight).abs() <= 1.0e-14);
            }
        }
    }

    #[test]
    fn spatial_filter_build_can_stop_before_first_solve() {
        let elements = (0..100)
            .map(|index| mock_element(FemPoint3::new(index as f64, 0.0, 0.0), 1.0))
            .collect::<Vec<_>>();
        let mut checks = 0usize;
        let filter = DensityFilter::new_bounded(&elements, 2.0, usize::MAX, || {
            checks += 1;
            checks > 10
        })
        .unwrap();
        assert!(filter.is_none());
        assert_eq!(checks, 11);
    }

    #[test]
    #[ignore = "explicit 50k-Tet4 preprocessing profile"]
    fn profile_product_scale_spatial_filter() {
        let elements = (0..50_000)
            .map(|index| {
                let x = index % 50;
                let y = (index / 50) % 50;
                let z = index / 2_500;
                mock_element(
                    FemPoint3::new(x as f64 * 2.4, y as f64 * 2.4, z as f64 * 2.4),
                    13.824,
                )
            })
            .collect::<Vec<_>>();
        let maximum_bytes = 512 * 1024 * 1024;
        let started = std::time::Instant::now();
        let filter = DensityFilter::new(&elements, 5.0, maximum_bytes).expect("50k filter");
        let elapsed_ms = started.elapsed().as_secs_f64() * 1_000.0;
        let neighbor_count = filter.rows.iter().map(Vec::len).sum::<usize>();
        let estimated_bytes = 2 * std::mem::size_of::<Vec<(usize, f64)>>() * elements.len()
            + 128 * elements.len()
            + 2 * std::mem::size_of::<(usize, f64)>() * neighbor_count;
        eprintln!(
            "{{:workload \"spatial-filter-50k\" :elements {} :neighbors {neighbor_count} :estimated-bytes {estimated_bytes} :elapsed-ms {elapsed_ms}}}",
            elements.len()
        );
        assert_eq!(filter.rows.len(), 50_000);
        assert!(estimated_bytes <= maximum_bytes);
        assert!(elapsed_ms <= 30_000.0);
    }

    #[test]
    #[ignore = "explicit product-scale topology preprocessing profile"]
    fn profile_product_scale_iteration_preprocessing() {
        let element_count = 50_287usize;
        let mut elements = (0..element_count)
            .map(|index| {
                let mut element = mock_element(
                    FemPoint3::new(
                        (index % 50) as f64 * 2.4,
                        ((index / 50) % 50) as f64 * 2.4,
                        (index / 2_500) as f64 * 2.4,
                    ),
                    13.824,
                );
                for local_node in 0..4 {
                    for axis in 0..3 {
                        element.dofs[local_node * 3 + axis] = (index + local_node) * 3 + axis;
                    }
                }
                for (row, values) in element.stiffness.iter_mut().enumerate() {
                    for (col, value) in values.iter_mut().enumerate() {
                        *value = if row == col { 4.0 } else { -0.01 };
                    }
                }
                element
            })
            .collect::<Vec<_>>();
        let dimension = (element_count + 3) * 3;
        let scales = vec![0.42; element_count];

        let started = std::time::Instant::now();
        let plan = StiffnessAssemblyPlan::new(dimension, &elements);
        let plan_ms = started.elapsed().as_secs_f64() * 1_000.0;
        let started = std::time::Instant::now();
        let matrix = plan.assemble(&elements, &scales);
        let assembly_ms = started.elapsed().as_secs_f64() * 1_000.0;
        let nnz = matrix.entries.len();

        let right_hand_sides = (0..5)
            .map(|load| {
                (0..dimension)
                    .map(|index| ((index + load) % 17) as f64 * 1.0e-3)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let constraints = (0..12)
            .map(|dof_index| FemDirichletConstraint {
                dof_index,
                value_mm: if dof_index % 2 == 0 { 0.0 } else { 0.025 },
            })
            .collect::<Vec<_>>();
        let started = std::time::Instant::now();
        let reduced = reduce_shared_dirichlet(
            &matrix,
            &right_hand_sides
                .iter()
                .map(Vec::as_slice)
                .collect::<Vec<_>>(),
            &constraints,
            &mut || false,
        )
        .unwrap()
        .unwrap();
        let reduction_ms = started.elapsed().as_secs_f64() * 1_000.0;
        assert_eq!(reduced.right_hand_sides.len(), 5);
        drop(reduced);
        drop(matrix);
        drop(plan);

        let filter = DensityFilter::new(&elements, 5.0, 512 * 1024 * 1024).unwrap();
        let volumes = elements
            .iter()
            .map(|element| element.volume)
            .collect::<Vec<_>>();
        let solid = (0..100).collect::<BTreeSet<_>>();
        let void = (100..200).collect::<BTreeSet<_>>();
        let started = std::time::Instant::now();
        let (gradient, fixed, total) =
            affine_physical_volume(&filter, &volumes, &solid, &void, 1.0e-3);
        let design = vec![0.42; element_count];
        let mut accumulated = 0.0;
        for _ in 0..96 {
            accumulated += affine_volume_fraction(&design, &gradient, fixed, total);
        }
        std::hint::black_box(accumulated);
        let affine_96_ms = started.elapsed().as_secs_f64() * 1_000.0;
        eprintln!(
            "{{:workload \"topology-preprocess-50287\" :elements {element_count} :dimension {dimension} :nnz {nnz} :rhs-count 5 :assembly-plan-ms {plan_ms} :assembly-ms {assembly_ms} :shared-reduction-ms {reduction_ms} :affine-volume-96-ms {affine_96_ms}}}"
        );
        assert!(plan_ms <= 30_000.0);
        assert!(assembly_ms <= 30_000.0);
        assert!(reduction_ms <= 30_000.0);
        assert!(affine_96_ms <= 30_000.0);
        elements.clear();
    }

    #[test]
    fn shared_dirichlet_reduction_matches_independent_reference_for_batched_rhs() {
        let matrix = FemSparseMatrix::from_dense(vec![
            vec![8.0, -2.0, 0.0, 1.0],
            vec![-2.0, 7.0, -1.0, 0.0],
            vec![0.0, -1.0, 6.0, -2.0],
            vec![1.0, 0.0, -2.0, 5.0],
        ])
        .unwrap();
        let right_hand_sides = [vec![1.0, 2.0, 3.0, 4.0], vec![-2.0, 1.0, 0.5, 8.0]];
        for prescribed in [0.0, 0.25] {
            let constraints = [FemDirichletConstraint {
                dof_index: 1,
                value_mm: prescribed,
            }];
            let references = right_hand_sides
                .iter()
                .map(|rhs| matrix.eliminate_dirichlet(rhs, &constraints).unwrap())
                .collect::<Vec<_>>();
            let reduced = reduce_shared_dirichlet(
                &matrix,
                &right_hand_sides
                    .iter()
                    .map(Vec::as_slice)
                    .collect::<Vec<_>>(),
                &constraints,
                &mut || false,
            )
            .unwrap()
            .unwrap();
            assert_eq!(reduced.matrix, references[0].matrix);
            assert_eq!(
                reduced.right_hand_sides,
                references
                    .iter()
                    .map(|item| item.rhs.clone())
                    .collect::<Vec<_>>()
            );
            assert_eq!(
                reduced.recover_full_solution(&[0.1, 0.2, 0.3]).unwrap(),
                references[0]
                    .recover_full_solution(&[0.1, 0.2, 0.3])
                    .unwrap()
            );
        }
    }

    #[test]
    fn affine_filtered_volume_matches_explicit_passive_masked_filter() {
        let elements = [
            mock_element(FemPoint3::new(0.0, 0.0, 0.0), 1.0),
            mock_element(FemPoint3::new(0.7, 0.0, 0.0), 2.0),
            mock_element(FemPoint3::new(1.4, 0.0, 0.0), 3.0),
            mock_element(FemPoint3::new(2.1, 0.0, 0.0), 4.0),
        ];
        let volumes = elements
            .iter()
            .map(|element| element.volume)
            .collect::<Vec<_>>();
        let filter = DensityFilter::new(&elements, 1.6, usize::MAX).unwrap();
        let solid = BTreeSet::from([0usize]);
        let void = BTreeSet::from([3usize]);
        let minimum_density = 1.0e-3;
        let design = [1.0, 0.42, 0.67, minimum_density];
        let explicit = volume_fraction(
            &physical_density(&filter, &design, &solid, &void, minimum_density),
            &volumes,
        );
        let (gradient, fixed, total) =
            affine_physical_volume(&filter, &volumes, &solid, &void, minimum_density);
        let affine = affine_volume_fraction(&design, &gradient, fixed, total);
        assert!((explicit - affine).abs() <= 1.0e-14);
    }

    #[test]
    fn initial_free_density_accounts_for_passive_solid_volume() {
        let gradient = [0.0, 1.0, 1.0];
        let free = [1usize, 2usize];
        let density = feasible_uniform_free_density(&gradient, &free, 1.0, 3.0, 0.5, 1.0e-3)
            .expect("feasible initial density");
        assert!((density - 0.25).abs() <= 1.0e-14);
        assert!(((1.0 + density * 2.0) / 3.0 - 0.5).abs() <= 1.0e-14);
    }

    #[test]
    fn gcmma_attempt_trace_uses_bounded_edn_records() {
        let trace = format_gcmma_attempt_trace(&[GcmmaAttemptTrace {
            outer_iteration: 2,
            inner_attempt: 3,
            exact_objective: 12.0,
            approximate_objective: 11.5,
            exact_constraint: -0.01,
            approximate_constraint: -0.02,
            objective_gap: 0.5,
            constraint_gap: 0.01,
            objective_lift: 4.0,
            constraint_lift: 0.25,
            dual: 8.0,
            maximum_density_change: 0.05,
        }]);

        assert_eq!(
            trace,
            "[{:outer 2 :inner 3 :exact-objective 1.200000e1 :approx-objective 1.150000e1 :exact-constraint -1.000000e-2 :approx-constraint -2.000000e-2 :objective-gap 5.000000e-1 :constraint-gap 1.000000e-2 :objective-lift 4.000000e0 :constraint-lift 2.500000e-1 :dual 8.000000e0 :max-density-change 5.000000e-2}]"
        );
    }

    #[test]
    fn gcmma_conservativeness_allows_solver_scale_roundoff_only() {
        let exact = 12.722_80;
        let approximate = exact - 3.3e-7;

        assert!(objective_is_conservative_with_solver_tolerance(
            exact,
            approximate,
            1.0e-8,
        ));
        assert!(!objective_is_conservative_with_solver_tolerance(
            exact,
            exact - 1.0e-4,
            1.0e-8,
        ));
    }

    #[test]
    fn gcmma_inner_bound_covers_observed_late_product_conservatization() {
        const { assert!(MAXIMUM_GCMMA_INNER_ATTEMPTS >= 24) };
    }

    #[test]
    fn solve_budget_is_scoped_to_requested_resume_work() {
        validate_advance_solve_budget(5, 9, 3_000).expect("nine resumed iterations fit");
        let error = validate_advance_solve_budget(5, 19, 3_000).unwrap_err();
        assert_eq!(error.field, "maximumSolveCount");
    }

    #[test]
    fn adjacency_pattern_assembly_matches_coordinate_tree_reference() {
        let mut first = mock_element(FemPoint3::new(0.0, 0.0, 0.0), 1.0);
        first.dofs = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
        let mut second = mock_element(FemPoint3::new(1.0, 0.0, 0.0), 1.0);
        second.dofs = [3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14];
        for row in 0..12 {
            for col in 0..12 {
                first.stiffness[row][col] = ((row + 1) * (col + 2)) as f64 / 17.0;
                second.stiffness[row][col] = ((row + 3) * (col + 1)) as f64 / 19.0;
            }
        }
        let elements = [first, second];
        let scales = [0.37, 0.81];
        let actual = assemble_scaled(15, &elements, &scales);
        let mut reference = BTreeMap::<(usize, usize), f64>::new();
        for (element, scale) in elements.iter().zip(scales) {
            for row in 0..12 {
                for col in 0..12 {
                    *reference
                        .entry((element.dofs[row], element.dofs[col]))
                        .or_default() += scale * element.stiffness[row][col];
                }
            }
        }
        let expected = FemSparseMatrix {
            dimension: 15,
            entries: reference
                .into_iter()
                .map(|((row, col), value)| FemSparseEntry { row, col, value })
                .collect(),
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn stiffness_assembly_plan_is_reusable_without_changing_entries() {
        let mut first = mock_element(FemPoint3::new(0.0, 0.0, 0.0), 1.0);
        first.dofs = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
        let mut second = mock_element(FemPoint3::new(1.0, 0.0, 0.0), 1.0);
        second.dofs = [3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14];
        let elements = [first, second];
        let plan = StiffnessAssemblyPlan::new(15, &elements);

        for scales in [[0.37, 0.81], [0.91, 0.23]] {
            assert_eq!(
                plan.assemble(&elements, &scales),
                assemble_scaled_reference(15, &elements, &scales),
            );
        }
    }

    #[test]
    fn parallel_sensitivity_preserves_serial_load_and_element_order() {
        let mut elements = [
            mock_element(FemPoint3::new(0.0, 0.0, 0.0), 1.0),
            mock_element(FemPoint3::new(1.0, 0.0, 0.0), 1.0),
        ];
        elements[0].dofs = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
        elements[1].dofs = [3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14];
        let solutions = vec![
            (0.7, (0..15).map(|value| value as f64 / 13.0).collect()),
            (1.3, (0..15).map(|value| value as f64 / 17.0).collect()),
        ];
        let densities = [0.42, 0.73];
        let actual = compliance_sensitivity(&elements, &solutions, &densities, 1.0e-3, 3.0);
        let expected = elements
            .iter()
            .enumerate()
            .map(|(index, element)| {
                solutions
                    .iter()
                    .fold(0.0, |gradient, (weight, displacement)| {
                        let u = element.dofs.map(|dof| displacement[dof]);
                        gradient
                            - weight
                                * simp_scale_derivative(densities[index], 1.0e-3, 3.0)
                                * quadratic(&element.stiffness, &u)
                    })
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
    }

    #[test]
    fn compliance_sensitivity_matches_central_finite_difference() {
        let mesh = FemIndexedTet4Mesh {
            schema_version: FEM_SCHEMA_VERSION,
            nodes: vec![
                FemPoint3::new(0.0, 0.0, 0.0),
                FemPoint3::new(1.0, 0.0, 0.0),
                FemPoint3::new(0.0, 1.0, 0.0),
                FemPoint3::new(0.0, 0.0, 1.0),
            ],
            cells: vec![[0, 1, 2, 3]],
        };
        let material = FemMaterial {
            schema_version: FEM_SCHEMA_VERSION,
            name: "test".into(),
            young_modulus_mpa: 4_000.0,
            poisson_ratio: 0.3,
            density_kg_per_mm3: 1.0e-6,
            yield_strength_mpa: 40.0,
        };
        let elements = element_data(&mesh, &material).unwrap();
        let constraints = (0..9)
            .map(|dof_index| FemDirichletConstraint {
                dof_index,
                value_mm: 0.0,
            })
            .collect::<Vec<_>>();
        let mut rhs = vec![0.0; 12];
        rhs[11] = -2.0;
        let loads = [FemTopologyLoadCase {
            id: "load".into(),
            weight: 1.0,
            rhs_n: rhs,
        }];
        let controls = FemTopologyControls {
            volume_fraction: 0.5,
            penalty: 3.0,
            minimum_density: 1.0e-3,
            filter_radius_mm: 1.0,
            move_limit: 0.1,
            convergence_tolerance: 1.0e-4,
            relative_solver_tolerance: 1.0e-8,
            require_parallel_solver: false,
            maximum_iterations: 2,
            maximum_dimension: 12,
            maximum_elements: 1,
            maximum_solve_count: 10,
            maximum_working_memory_bytes: 10_000_000,
            maximum_result_bytes: 100_000,
            maximum_wall_time_ms: 1_000,
            runtime_identity_digest: "sha256:test".into(),
            passive_solid_cells: vec![],
            passive_void_cells: vec![],
        };
        let rho = 0.53;
        let epsilon = 1.0e-6;
        let (compliance, gradient, _) = analyze(
            mesh.nodes.len(),
            &elements,
            &material,
            &loads,
            &constraints,
            &[rho],
            &controls,
        )
        .unwrap();
        assert!(compliance > 0.0);
        let plus = analyze(
            mesh.nodes.len(),
            &elements,
            &material,
            &loads,
            &constraints,
            &[rho + epsilon],
            &controls,
        )
        .unwrap()
        .0;
        let minus = analyze(
            mesh.nodes.len(),
            &elements,
            &material,
            &loads,
            &constraints,
            &[rho - epsilon],
            &controls,
        )
        .unwrap()
        .0;
        let finite_difference = (plus - minus) / (2.0 * epsilon);
        let relative_error = (gradient[0] - finite_difference).abs() / finite_difference.abs();
        assert!(relative_error <= 1.0e-6, "relative error {relative_error}");
    }

    fn mock_element(centroid: FemPoint3, volume: f64) -> ElementData {
        ElementData {
            dofs: [0; 12],
            stiffness: [[0.0; 12]; 12],
            volume,
            centroid,
        }
    }
}
