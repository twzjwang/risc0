// Copyright 2023 RISC Zero, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use risc0_zkp::{
    core::{digest::Digest, hash::HashSuite},
    field::baby_bear::{BabyBear, BabyBearElem},
    hal::{cpu::CpuHal, Hal},
    prove::poly_group::PolyGroup,
};

use super::{RECURSION_CODE_SIZE, RECURSION_PO2};

/// A Program for the recursion circuit (e.g. lift_20 or join).
///
/// The recursion circuit is an application specific virtual machine with a limited instruction
/// set, no control flow operations, and a write-once memory tape. Although it is not general
/// purpose, it can load and execute a program, similar to the rv32im zkVM.
///
/// Programs for the recursion circuit are loaded into the control columns, which is a set of
/// public columns in the witness. Programs are therefore identified by their control ID, which is
/// similar but not the same as the image ID used to identify rv32im programs.
#[derive(Clone)]
pub struct Program {
    /// The code of the program, encoded as Baby Bear field elements.
    pub code: Vec<BabyBearElem>,

    /// The number of code columns.
    pub code_size: usize,
}

impl Program {
    /// Create a [Program] from a stream of data encoded by Zirgen.
    pub fn from_encoded(encoded: &[u32]) -> Self {
        let prog = Self {
            code: encoded.iter().copied().map(BabyBearElem::from).collect(),
            code_size: RECURSION_CODE_SIZE,
        };
        assert_eq!(prog.code.len() % RECURSION_CODE_SIZE, 0);
        prog
    }

    /// Total number of rows in the code group for this program.
    pub fn code_rows(&self) -> usize {
        self.code.len() / self.code_size
    }

    /// An iterator over the rows of the code group.
    pub fn code_by_row(&self) -> impl Iterator<Item = &[BabyBearElem]> {
        self.code.as_slice().chunks(self.code_size)
    }

    /// Given a [Program] for the recursion circuit, compute the control ID as the FRI Merkle root
    /// of the code group. This uniquely identifies the program running on the recursion circuit
    /// (e.g. lift_20 or join)
    pub fn compute_control_id(&self, hash_suite: HashSuite<BabyBear>) -> Digest {
        let hal = CpuHal::new(hash_suite);
        let cycles = 1 << RECURSION_PO2;

        let mut code = vec![BabyBearElem::default(); cycles * self.code_size];

        for (cycle, row) in self.code_by_row().enumerate() {
            for (i, elem) in row.iter().enumerate() {
                code[cycles * i + cycle] = *elem;
            }
        }
        let coeffs = hal.copy_from_elem("coeffs", &code);
        // Do interpolate & shift
        hal.batch_interpolate_ntt(&coeffs, self.code_size);
        hal.zk_shift(&coeffs, self.code_size);
        // Make the poly-group & extract the root
        let code_group = PolyGroup::new(&hal, coeffs, self.code_size, cycles, "code");
        let root = *code_group.merkle.root();
        tracing::trace!("Computed recursion code: {root:?}");
        root
    }
}
