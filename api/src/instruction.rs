use bytemuck::{Pod, Zeroable};
use drillx::Solution;
use num_enum::TryFromPrimitive;
use ore_utils::{impl_instruction_from_bytes, impl_to_bytes};
use solana_program::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, TryFromPrimitive)]
#[rustfmt::skip]
pub enum PoolInstruction {
    // User
    Open = 0,
    Claim = 1,
    Stake = 2,
    
    // Operator
    Launch = 200,
    Attribute = 100,
    Submit = 102,
}

impl PoolInstruction {
    pub fn to_vec(&self) -> Vec<u8> {
        vec![*self as u8]
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct AttributeArgs {
    pub balance: [u8; 8],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct ClaimArgs {
    pub amount: [u8; 8],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct LaunchArgs {
    pub pool_bump: u8,
    pub proof_bump: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct OpenArgs {
    pub member_bump: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct SubmitArgs {
    pub attestation: [u8; 32],
    pub digest: [u8; 16],
    pub nonce: [u8; 8],
}

impl_to_bytes!(LaunchArgs);
impl_to_bytes!(ClaimArgs);
impl_to_bytes!(AttributeArgs);
impl_to_bytes!(OpenArgs);
impl_to_bytes!(SubmitArgs);

impl_instruction_from_bytes!(LaunchArgs);
impl_instruction_from_bytes!(ClaimArgs);
impl_instruction_from_bytes!(OpenArgs);
impl_instruction_from_bytes!(AttributeArgs);
impl_instruction_from_bytes!(SubmitArgs);

/// Builds an initialize instruction.
pub fn initialize(signer: Pubkey) -> Instruction {
    Instruction {
        program_id: crate::id(),
        accounts: vec![AccountMeta::new(signer, true)],
        data: [PoolInstruction::Launch.to_vec()].concat(),
    }
}

/// Builds an submit instruction.
pub fn submit(signer: Pubkey, solution: Solution, attestation: [u8; 32]) -> Instruction {
    Instruction {
        program_id: crate::id(),
        accounts: vec![AccountMeta::new(signer, true)],
        data: [
            PoolInstruction::Submit.to_vec(),
            SubmitArgs {
                attestation,
                digest: solution.d,
                nonce: solution.n,
            }
            .to_bytes()
            .to_vec(),
        ]
        .concat(),
    }
}
