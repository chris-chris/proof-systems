use std::ops::{Index, IndexMut};

use ark_ff::{One, Zero};
use kimchi::circuits::polynomials::keccak::constants::{
    CHI_SHIFTS_B_OFF, CHI_SHIFTS_SUM_OFF, PIRHO_DENSE_E_OFF, PIRHO_DENSE_ROT_E_OFF,
    PIRHO_EXPAND_ROT_E_OFF, PIRHO_QUOTIENT_E_OFF, PIRHO_REMAINDER_E_OFF, PIRHO_SHIFTS_E_OFF,
    QUARTERS, RATE_IN_BYTES, SPONGE_BYTES_OFF, SPONGE_NEW_STATE_OFF, SPONGE_SHIFTS_OFF,
    THETA_DENSE_C_OFF, THETA_DENSE_ROT_C_OFF, THETA_EXPAND_ROT_C_OFF, THETA_QUOTIENT_C_OFF,
    THETA_REMAINDER_C_OFF, THETA_SHIFTS_C_OFF,
};
use rayon::iter::{FromParallelIterator, IntoParallelIterator, ParallelIterator};

use super::{ZKVM_KECCAK_COLS_CURR, ZKVM_KECCAK_COLS_NEXT};

const MODE_FLAGS_COLS_LENGTH: usize = 7;
const SUFFIX_COLS_LENGTH: usize = 5;
const ZKVM_KECCAK_COLS_LENGTH: usize = ZKVM_KECCAK_COLS_CURR
    + ZKVM_KECCAK_COLS_NEXT
    + QUARTERS
    + RATE_IN_BYTES
    + SUFFIX_COLS_LENGTH
    + MODE_FLAGS_COLS_LENGTH
    + 2;

const FLAG_ROUND_OFFSET: usize = 0;
const FLAG_ABSORB_OFFSET: usize = 1;
const FLAG_SQUEEZE_OFFSET: usize = 2;
const FLAG_ROOT_OFFSET: usize = 3;
const FLAG_PAD_LENGTH_OFFSET: usize = 4;
const FLAG_INV_PAD_LENGTH_OFFSET: usize = 5;
const FLAG_TWO_TO_PAD_OFFSET: usize = 6;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeccakColumn {
    HashIndex,
    StepIndex,
    FlagRound,              // Coeff Round = [0..24)
    FlagAbsorb,             // Coeff Absorb = 0 | 1
    FlagSqueeze,            // Coeff Squeeze = 0 | 1
    FlagRoot,               // Coeff Root = 0 | 1
    PadLength,              // Coeff Length 0 | 1 ..=136
    InvPadLength,           // Inverse of PadLength when PadLength != 0
    TwoToPad,               // 2^PadLength
    PadBytesFlags(usize),   // 136 boolean values
    PadSuffix(usize),       // 5 values with padding suffix
    RoundConstants(usize),  // Round constants
    Input(usize),           // Curr[0..100) either ThetaStateA or SpongeOldState
    ThetaShiftsC(usize),    // Round Curr[100..180)
    ThetaDenseC(usize),     // Round Curr[180..200)
    ThetaQuotientC(usize),  // Round Curr[200..205)
    ThetaRemainderC(usize), // Round Curr[205..225)
    ThetaDenseRotC(usize),  // Round Curr[225..245)
    ThetaExpandRotC(usize), // Round Curr[245..265)
    PiRhoShiftsE(usize),    // Round Curr[265..665)
    PiRhoDenseE(usize),     // Round Curr[665..765)
    PiRhoQuotientE(usize),  // Round Curr[765..865)
    PiRhoRemainderE(usize), // Round Curr[865..965)
    PiRhoDenseRotE(usize),  // Round Curr[965..1065)
    PiRhoExpandRotE(usize), // Round Curr[1065..1165)
    ChiShiftsB(usize),      // Round Curr[1165..1565)
    ChiShiftsSum(usize),    // Round Curr[1565..1965)
    SpongeNewState(usize),  // Sponge Curr[100..200)
    SpongeBytes(usize),     // Sponge Curr[200..400)
    SpongeShifts(usize),    // Sponge Curr[400..800)
    Output(usize),          // Next[0..100) either IotaStateG or SpongeXorState
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeccakColumns<T> {
    pub hash_index: T,
    pub step_index: T,
    pub mode_flags: [T; MODE_FLAGS_COLS_LENGTH], // Round, Absorb, Squeeze, Root, PadLength, InvPadLength, TwoToPad
    pub pad_bytes_flags: [T; RATE_IN_BYTES],     // 136 boolean values -> sponge
    pub pad_suffix: [T; SUFFIX_COLS_LENGTH],     // 5 values with padding suffix -> sponge
    pub round_constants: [T; QUARTERS],          // Round constants -> round
    pub curr: [T; ZKVM_KECCAK_COLS_CURR],        // Curr[0..1965)
    pub next: [T; ZKVM_KECCAK_COLS_NEXT],        // Next[0..100)
}

impl<T: Clone> KeccakColumns<T> {
    pub fn chunk(&self, offset: usize, length: usize) -> &[T] {
        &self.curr[offset..offset + length]
    }
}

impl<T: Zero + One + Clone> Default for KeccakColumns<T> {
    fn default() -> Self {
        KeccakColumns {
            hash_index: T::zero(),
            step_index: T::zero(),
            mode_flags: std::array::from_fn(|_| T::zero()), // Defaults are zero, but lookups will not be triggered
            pad_bytes_flags: std::array::from_fn(|_| T::zero()),
            pad_suffix: std::array::from_fn(|_| T::zero()),
            round_constants: std::array::from_fn(|_| T::zero()), // default zeros, but lookup only if is round
            curr: std::array::from_fn(|_| T::zero()),
            next: std::array::from_fn(|_| T::zero()),
        }
    }
}

impl<T: Clone> Index<KeccakColumn> for KeccakColumns<T> {
    type Output = T;

    fn index(&self, index: KeccakColumn) -> &Self::Output {
        match index {
            KeccakColumn::HashIndex => &self.hash_index,
            KeccakColumn::StepIndex => &self.step_index,
            KeccakColumn::FlagRound => &self.mode_flags[FLAG_ROUND_OFFSET],
            KeccakColumn::FlagAbsorb => &self.mode_flags[FLAG_ABSORB_OFFSET],
            KeccakColumn::FlagSqueeze => &self.mode_flags[FLAG_SQUEEZE_OFFSET],
            KeccakColumn::FlagRoot => &self.mode_flags[FLAG_ROOT_OFFSET],
            KeccakColumn::PadLength => &self.mode_flags[FLAG_PAD_LENGTH_OFFSET],
            KeccakColumn::InvPadLength => &self.mode_flags[FLAG_INV_PAD_LENGTH_OFFSET],
            KeccakColumn::TwoToPad => &self.mode_flags[FLAG_TWO_TO_PAD_OFFSET],
            KeccakColumn::PadBytesFlags(idx) => &self.pad_bytes_flags[idx],
            KeccakColumn::PadSuffix(idx) => &self.pad_suffix[idx],
            KeccakColumn::RoundConstants(idx) => &self.round_constants[idx],
            KeccakColumn::Input(idx) => &self.curr[idx],
            KeccakColumn::ThetaShiftsC(idx) => &self.curr[THETA_SHIFTS_C_OFF + idx],
            KeccakColumn::ThetaDenseC(idx) => &self.curr[THETA_DENSE_C_OFF + idx],
            KeccakColumn::ThetaQuotientC(idx) => &self.curr[THETA_QUOTIENT_C_OFF + idx],
            KeccakColumn::ThetaRemainderC(idx) => &self.curr[THETA_REMAINDER_C_OFF + idx],
            KeccakColumn::ThetaDenseRotC(idx) => &self.curr[THETA_DENSE_ROT_C_OFF + idx],
            KeccakColumn::ThetaExpandRotC(idx) => &self.curr[THETA_EXPAND_ROT_C_OFF + idx],
            KeccakColumn::PiRhoShiftsE(idx) => &self.curr[PIRHO_SHIFTS_E_OFF + idx],
            KeccakColumn::PiRhoDenseE(idx) => &self.curr[PIRHO_DENSE_E_OFF + idx],
            KeccakColumn::PiRhoQuotientE(idx) => &self.curr[PIRHO_QUOTIENT_E_OFF + idx],
            KeccakColumn::PiRhoRemainderE(idx) => &self.curr[PIRHO_REMAINDER_E_OFF + idx],
            KeccakColumn::PiRhoDenseRotE(idx) => &self.curr[PIRHO_DENSE_ROT_E_OFF + idx],
            KeccakColumn::PiRhoExpandRotE(idx) => &self.curr[PIRHO_EXPAND_ROT_E_OFF + idx],
            KeccakColumn::ChiShiftsB(idx) => &self.curr[CHI_SHIFTS_B_OFF + idx],
            KeccakColumn::ChiShiftsSum(idx) => &self.curr[CHI_SHIFTS_SUM_OFF + idx],
            KeccakColumn::SpongeNewState(idx) => &self.curr[SPONGE_NEW_STATE_OFF + idx],
            KeccakColumn::SpongeBytes(idx) => &self.curr[SPONGE_BYTES_OFF + idx],
            KeccakColumn::SpongeShifts(idx) => &self.curr[SPONGE_SHIFTS_OFF + idx],
            KeccakColumn::Output(idx) => &self.next[idx],
        }
    }
}

impl<T: Clone> IndexMut<KeccakColumn> for KeccakColumns<T> {
    fn index_mut(&mut self, index: KeccakColumn) -> &mut Self::Output {
        match index {
            KeccakColumn::HashIndex => &mut self.hash_index,
            KeccakColumn::StepIndex => &mut self.step_index,
            KeccakColumn::FlagRound => &mut self.mode_flags[FLAG_ROUND_OFFSET],
            KeccakColumn::FlagAbsorb => &mut self.mode_flags[FLAG_ABSORB_OFFSET],
            KeccakColumn::FlagSqueeze => &mut self.mode_flags[FLAG_SQUEEZE_OFFSET],
            KeccakColumn::FlagRoot => &mut self.mode_flags[FLAG_ROOT_OFFSET],
            KeccakColumn::PadLength => &mut self.mode_flags[FLAG_PAD_LENGTH_OFFSET],
            KeccakColumn::InvPadLength => &mut self.mode_flags[FLAG_INV_PAD_LENGTH_OFFSET],
            KeccakColumn::TwoToPad => &mut self.mode_flags[FLAG_TWO_TO_PAD_OFFSET],
            KeccakColumn::PadBytesFlags(idx) => &mut self.pad_bytes_flags[idx],
            KeccakColumn::PadSuffix(idx) => &mut self.pad_suffix[idx],
            KeccakColumn::RoundConstants(idx) => &mut self.round_constants[idx],
            KeccakColumn::Input(idx) => &mut self.curr[idx],
            KeccakColumn::ThetaShiftsC(idx) => &mut self.curr[THETA_SHIFTS_C_OFF + idx],
            KeccakColumn::ThetaDenseC(idx) => &mut self.curr[THETA_DENSE_C_OFF + idx],
            KeccakColumn::ThetaQuotientC(idx) => &mut self.curr[THETA_QUOTIENT_C_OFF + idx],
            KeccakColumn::ThetaRemainderC(idx) => &mut self.curr[THETA_REMAINDER_C_OFF + idx],
            KeccakColumn::ThetaDenseRotC(idx) => &mut self.curr[THETA_DENSE_ROT_C_OFF + idx],
            KeccakColumn::ThetaExpandRotC(idx) => &mut self.curr[THETA_EXPAND_ROT_C_OFF + idx],
            KeccakColumn::PiRhoShiftsE(idx) => &mut self.curr[PIRHO_SHIFTS_E_OFF + idx],
            KeccakColumn::PiRhoDenseE(idx) => &mut self.curr[PIRHO_DENSE_E_OFF + idx],
            KeccakColumn::PiRhoQuotientE(idx) => &mut self.curr[PIRHO_QUOTIENT_E_OFF + idx],
            KeccakColumn::PiRhoRemainderE(idx) => &mut self.curr[PIRHO_REMAINDER_E_OFF + idx],
            KeccakColumn::PiRhoDenseRotE(idx) => &mut self.curr[PIRHO_DENSE_ROT_E_OFF + idx],
            KeccakColumn::PiRhoExpandRotE(idx) => &mut self.curr[PIRHO_EXPAND_ROT_E_OFF + idx],
            KeccakColumn::ChiShiftsB(idx) => &mut self.curr[CHI_SHIFTS_B_OFF + idx],
            KeccakColumn::ChiShiftsSum(idx) => &mut self.curr[CHI_SHIFTS_SUM_OFF + idx],
            KeccakColumn::SpongeNewState(idx) => &mut self.curr[SPONGE_NEW_STATE_OFF + idx],
            KeccakColumn::SpongeBytes(idx) => &mut self.curr[SPONGE_BYTES_OFF + idx],
            KeccakColumn::SpongeShifts(idx) => &mut self.curr[SPONGE_SHIFTS_OFF + idx],
            KeccakColumn::Output(idx) => &mut self.next[idx],
        }
    }
}

impl<F> IntoIterator for KeccakColumns<F> {
    type Item = F;
    type IntoIter = std::vec::IntoIter<F>;

    fn into_iter(self) -> Self::IntoIter {
        let mut iter_contents = Vec::with_capacity(ZKVM_KECCAK_COLS_LENGTH);
        iter_contents.push(self.hash_index);
        iter_contents.push(self.step_index);
        iter_contents.extend(self.mode_flags);
        iter_contents.extend(self.pad_bytes_flags);
        iter_contents.extend(self.pad_suffix);
        iter_contents.extend(self.round_constants);
        iter_contents.extend(self.curr);
        iter_contents.extend(self.next);
        iter_contents.into_iter()
    }
}

impl<G> IntoParallelIterator for KeccakColumns<G>
where
    Vec<G>: IntoParallelIterator,
{
    type Iter = <Vec<G> as IntoParallelIterator>::Iter;
    type Item = <Vec<G> as IntoParallelIterator>::Item;

    fn into_par_iter(self) -> Self::Iter {
        let mut iter_contents = Vec::with_capacity(ZKVM_KECCAK_COLS_LENGTH);
        iter_contents.push(self.hash_index);
        iter_contents.push(self.step_index);
        iter_contents.extend(self.mode_flags);
        iter_contents.extend(self.pad_bytes_flags);
        iter_contents.extend(self.pad_suffix);
        iter_contents.extend(self.round_constants);
        iter_contents.extend(self.curr);
        iter_contents.extend(self.next);
        iter_contents.into_par_iter()
    }
}

impl<G: Send + std::fmt::Debug> FromParallelIterator<G> for KeccakColumns<G> {
    fn from_par_iter<I>(par_iter: I) -> Self
    where
        I: IntoParallelIterator<Item = G>,
    {
        let mut iter_contents = par_iter.into_par_iter().collect::<Vec<_>>();
        let next = iter_contents
            .drain(iter_contents.len() - ZKVM_KECCAK_COLS_NEXT..)
            .collect::<Vec<G>>()
            .try_into()
            .unwrap();
        let curr = iter_contents
            .drain(iter_contents.len() - ZKVM_KECCAK_COLS_CURR..)
            .collect::<Vec<G>>()
            .try_into()
            .unwrap();
        let round_constants = iter_contents
            .drain(iter_contents.len() - QUARTERS..)
            .collect::<Vec<G>>()
            .try_into()
            .unwrap();
        let pad_suffix = iter_contents
            .drain(iter_contents.len() - SUFFIX_COLS_LENGTH..)
            .collect::<Vec<G>>()
            .try_into()
            .unwrap();
        let pad_bytes_flags = iter_contents
            .drain(iter_contents.len() - RATE_IN_BYTES..)
            .collect::<Vec<G>>()
            .try_into()
            .unwrap();
        let mode_flags = iter_contents
            .drain(iter_contents.len() - MODE_FLAGS_COLS_LENGTH..)
            .collect::<Vec<G>>()
            .try_into()
            .unwrap();
        let step_index = iter_contents.pop().unwrap();
        let hash_index = iter_contents.pop().unwrap();
        KeccakColumns {
            hash_index,
            step_index,
            mode_flags,
            pad_bytes_flags,
            pad_suffix,
            round_constants,
            curr,
            next,
        }
    }
}

impl<'data, G> IntoParallelIterator for &'data KeccakColumns<G>
where
    Vec<&'data G>: IntoParallelIterator,
{
    type Iter = <Vec<&'data G> as IntoParallelIterator>::Iter;
    type Item = <Vec<&'data G> as IntoParallelIterator>::Item;

    fn into_par_iter(self) -> Self::Iter {
        let mut iter_contents = Vec::with_capacity(ZKVM_KECCAK_COLS_LENGTH);
        iter_contents.push(&self.hash_index);
        iter_contents.push(&self.step_index);
        iter_contents.extend(&self.mode_flags);
        iter_contents.extend(&self.pad_bytes_flags);
        iter_contents.extend(&self.pad_suffix);
        iter_contents.extend(&self.round_constants);
        iter_contents.extend(&self.curr);
        iter_contents.extend(&self.next);
        iter_contents.into_par_iter()
    }
}

impl<'data, G> IntoParallelIterator for &'data mut KeccakColumns<G>
where
    Vec<&'data mut G>: IntoParallelIterator,
{
    type Iter = <Vec<&'data mut G> as IntoParallelIterator>::Iter;
    type Item = <Vec<&'data mut G> as IntoParallelIterator>::Item;

    fn into_par_iter(self) -> Self::Iter {
        let mut iter_contents = Vec::with_capacity(ZKVM_KECCAK_COLS_LENGTH);
        iter_contents.push(&mut self.hash_index);
        iter_contents.push(&mut self.step_index);
        iter_contents.extend(&mut self.mode_flags);
        iter_contents.extend(&mut self.pad_bytes_flags);
        iter_contents.extend(&mut self.pad_suffix);
        iter_contents.extend(&mut self.round_constants);
        iter_contents.extend(&mut self.curr);
        iter_contents.extend(&mut self.next);
        iter_contents.into_par_iter()
    }
}
