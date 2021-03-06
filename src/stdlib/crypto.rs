use std::{cell::RefCell, rc::Rc};

use ark_ec::AffineCurve as _;
use ark_ff::Zero;
use kimchi::{
    circuits::polynomials::poseidon::{POS_ROWS_PER_HASH, ROUNDS_PER_ROW},
    commitment_dlog::{commitment::CommitmentCurve as _, srs::endos},
    mina_curves::pasta::vesta,
    oracle::{
        self,
        constants::{PlonkSpongeConstantsKimchi, SpongeConstants},
        permutation::full_round,
    },
};

use crate::{
    ast::{CellVars, Compiler, FuncType, GateKind, Value, Var},
    constants::{self, Span},
    field::Field,
};

const POSEIDON_FN: &str = "poseidon(input: [Field; 2]) -> [Field; 3]";

pub const CRYPTO_FNS: [(&str, FuncType); 1] = [(POSEIDON_FN, poseidon)];

pub fn poseidon(compiler: &mut Compiler, vars: &[Var], span: Span) -> Option<Var> {
    // double check input
    assert_eq!(vars.len(), 1);
    let input = match vars[0].circuit_var() {
        None => unimplemented!(),
        Some(cvar) => cvar.vars,
    };
    assert_eq!(input.len(), 2);

    // get constants needed for poseidon
    let (endo_q, _endo_r) = endos::<vesta::Affine>();
    let base = vesta::Affine::prime_subgroup_generator()
        .to_coordinates()
        .unwrap();
    let poseidon_params = oracle::pasta::fp_kimchi::params();

    let rc = &poseidon_params.round_constants;
    let width = PlonkSpongeConstantsKimchi::SPONGE_WIDTH;

    // pad the input (for the capacity)
    let zero = compiler.constant(Field::zero(), span);
    let mut input = input.clone();
    input.push(zero);

    let mut states = vec![input.clone()];

    // 0..11
    for row in 0..POS_ROWS_PER_HASH {
        let offset = row * ROUNDS_PER_ROW; // row * 5

        // 0..5
        for i in 0..ROUNDS_PER_ROW {
            let mut new_state = vec![];

            let prev_0 = states[states.len() - 1][0];
            let prev_1 = states[states.len() - 1][1];
            let prev_2 = states[states.len() - 1][2];

            for col in 0..3 {
                // create each variable
                let var = compiler.new_internal_var(Value::Hint(Box::new(move |compiler, env| {
                    let x1 = compiler.compute_var(env, prev_0)?;
                    let x2 = compiler.compute_var(env, prev_1)?;
                    let x3 = compiler.compute_var(env, prev_2)?;

                    let mut acc = vec![x1, x2, x3];

                    // Do one full round on the previous value
                    full_round::<Field, PlonkSpongeConstantsKimchi>(
                        &oracle::pasta::fp_kimchi::params(),
                        &mut acc,
                        offset + i,
                    );

                    Ok(acc[col])
                })));

                new_state.push(var);
            }

            states.push(new_state);
        }

        let coeffs = (0..constants::NUM_REGISTERS)
            .map(|i| rc[offset + (i / width)][i % width])
            .collect();

        let vars = vec![
            Some(states[offset][0]),
            Some(states[offset][1]),
            Some(states[offset][2]),
            Some(states[offset + 4][0]),
            Some(states[offset + 4][1]),
            Some(states[offset + 4][2]),
            Some(states[offset + 1][0]),
            Some(states[offset + 1][1]),
            Some(states[offset + 1][2]),
            Some(states[offset + 2][0]),
            Some(states[offset + 2][1]),
            Some(states[offset + 2][2]),
            Some(states[offset + 3][0]),
            Some(states[offset + 3][1]),
            Some(states[offset + 3][2]),
        ];

        compiler.gates(GateKind::Poseidon, vars, coeffs, span);
    }

    let final_state = &states[states.len() - 1];
    let final_row = vec![
        Some(final_state[0]),
        Some(final_state[1]),
        Some(final_state[2]),
    ];

    // zero gate to store the result
    compiler.gates(GateKind::DoubleGeneric, final_row.clone(), vec![], span);

    //    states.borrow_mut().pop().unwrap();
    let vars = final_row.iter().flatten().cloned().collect();
    Some(Var::new_circuit_var(vars, span))
}
