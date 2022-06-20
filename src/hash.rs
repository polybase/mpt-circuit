//! The hash circuit base on poseidon.

use crate::poseidon::primitives::{ConstantLengthIden3, Hash, P128Pow5T3};
use halo2_proofs::pairing::bn256::Fr;

/// indicate an field can be hashed in merkle tree (2 Fields to 1 Field)
pub trait Hashable: Sized {
    /// execute hash for any sequence of fields
    fn hash(inp: [Self; 2]) -> Self;
}

type Poseidon = Hash<Fr, P128Pow5T3<Fr>, ConstantLengthIden3<2>, 3, 2>;

impl Hashable for Fr {
    fn hash(inp: [Self; 2]) -> Self {
        Poseidon::init().hash(inp)
    }
}

use crate::poseidon::{PoseidonInstructions, Pow5Chip, Pow5Config, StateWord, Var};
use halo2_proofs::{
    circuit::{Layouter, SimpleFloorPlanner},
    plonk::{Advice, Circuit, Column, ConstraintSystem, Error, Fixed},
};

/// The config for hash circuit
#[derive(Clone, Debug)]
pub struct HashConfig {
    permute_config: Pow5Config<Fr, 3, 2>,
    hash_table: [Column<Advice>; 3],
    constants: [Column<Fixed>; 6],
}

/// Hash circuit
pub struct HashCircuit<const CALCS: usize> {
    /// the input messages for hashes
    pub inputs: [Option<[Fr; 2]>; CALCS],
}

impl<const CALCS: usize> Circuit<Fr> for HashCircuit<CALCS> {
    type Config = HashConfig;
    type FloorPlanner = SimpleFloorPlanner;

    fn without_witnesses(&self) -> Self {
        Self {
            inputs: [None; CALCS],
        }
    }

    fn configure(meta: &mut ConstraintSystem<Fr>) -> Self::Config {
        let state = [0; 3].map(|_| meta.advice_column());
        let partial_sbox = meta.advice_column();
        let constants = [0; 6].map(|_| meta.fixed_column());

        let hash_table = [0; 3].map(|_| meta.advice_column());
        for col in hash_table {
            meta.enable_equality(col);
        }
        meta.enable_equality(constants[0]);

        HashConfig {
            permute_config: Pow5Chip::configure::<P128Pow5T3<Fr>>(
                meta,
                state,
                partial_sbox,
                constants[..3].try_into().unwrap(), //rc_a
                constants[3..].try_into().unwrap(), //rc_b
            ),
            hash_table,
            constants,
        }
    }

    fn synthesize(
        &self,
        config: Self::Config,
        mut layouter: impl Layouter<Fr>,
    ) -> Result<(), Error> {
        let constant_cells = layouter.assign_region(
            || "constant heading",
            |mut region| {
                let c0 = region.assign_fixed(
                    || "constant zero",
                    config.constants[0],
                    0,
                    || Ok(Fr::zero()),
                )?;

                Ok([StateWord::from(c0)])
            },
        )?;

        let zero_cell = &constant_cells[0];

        let (states, hashes) = layouter.assign_region(
            || "hash table",
            |mut region| {
                let mut states = Vec::new();
                let mut hashes = Vec::new();

                for (i, inp) in self.inputs.into_iter().enumerate() {
                    let inp = inp.unwrap_or_else(|| [Fr::zero(), Fr::zero()]);

                    let c1 = region.assign_advice(
                        || format!("hash input first_{}", i),
                        config.hash_table[0],
                        i,
                        || Ok(inp[0]),
                    )?;

                    let c2 = region.assign_advice(
                        || format!("hash input second_{}", i),
                        config.hash_table[1],
                        i,
                        || Ok(inp[1]),
                    )?;

                    let c3 = region.assign_advice(
                        || format!("hash output_{}", i),
                        config.hash_table[2],
                        i,
                        || Ok(Poseidon::init().hash(inp)),
                    )?;

                    //we directly specify the init state of permutation
                    states.push([zero_cell.clone(), StateWord::from(c1), StateWord::from(c2)]);
                    hashes.push(StateWord::from(c3));
                }

                Ok((states, hashes))
            },
        )?;

        let mut chip_finals = Vec::new();

        for state in states {
            let chip = Pow5Chip::construct(config.permute_config.clone());

            let final_state = <Pow5Chip<_, 3, 2> as PoseidonInstructions<
                Fr,
                P128Pow5T3<Fr>,
                3,
                2,
            >>::permute(&chip, &mut layouter, &state)?;

            chip_finals.push(final_state);
        }

        layouter.assign_region(
            || "final state dummy",
            |mut region| {
                for (hash, final_state) in hashes.iter().zip(chip_finals.iter()) {
                    region.constrain_equal(hash.cell(), final_state[0].cell())?;
                }

                Ok(())
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ff::PrimeField;

    #[test]
    fn poseidon_hash() {
        let b1: Fr = Fr::from_str_vartime("1").unwrap();
        let b2: Fr = Fr::from_str_vartime("2").unwrap();

        let h = Fr::hash([b1, b2]);
        assert_eq!(
            h.to_string(),
            "0x115cc0f5e7d690413df64c6b9662e9cf2a3617f2743245519e19607a4417189a" // "7853200120776062878684798364095072458815029376092732009249414926327459813530"
        );
    }

    use halo2_proofs::dev::MockProver;

    #[cfg(feature = "print_layout")]
    #[test]
    fn print_circuit() {
        use plotters::prelude::*;

        let root = BitMapBackend::new("hash-layout.png", (1024, 768)).into_drawing_area();
        root.fill(&WHITE).unwrap();
        let root = root
            .titled("Hash circuit Layout", ("sans-serif", 60))
            .unwrap();

        let circuit = HashCircuit::<1> { inputs: [None] };
        halo2_proofs::dev::CircuitLayout::default()
            .show_equality_constraints(true)
            .render(6, &circuit, &root)
            .unwrap();
    }

    #[test]
    fn poseidon_hash_circuit() {
        let message = [
            Fr::from_str_vartime("1").unwrap(),
            Fr::from_str_vartime("2").unwrap(),
        ];

        let k = 6;
        let circuit = HashCircuit::<1> {
            inputs: [Some(message)],
        };
        let prover = MockProver::run(k, &circuit, vec![]).unwrap();
        assert_eq!(prover.verify(), Ok(()));
    }
}