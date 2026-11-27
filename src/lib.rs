//! Token escrow with the instruction and account layout of the Anchor escrow.
#![no_std]

mod accounts;
mod instructions;
pub mod state;
mod token;

use pinocchio::{error::ProgramError, AccountView, Address, ProgramResult};

pub const ESCROW_SEED: &[u8] = b"escrow";
pub const MAKE: [u8; 8] = [138, 227, 232, 77, 223, 166, 96, 197];
pub const TAKE: [u8; 8] = [149, 226, 52, 104, 6, 142, 230, 39];
pub const REFUND: [u8; 8] = [2, 96, 183, 251, 63, 208, 46, 46];

#[repr(u32)]
enum EscrowError {
    ZeroAmount = 6000,
    SameMint,
    InvalidMaker,
    InvalidMintA,
    InvalidMintB,
}

impl From<EscrowError> for ProgramError {
    fn from(error: EscrowError) -> Self {
        Self::Custom(error as u32)
    }
}

#[cfg(not(feature = "no-entrypoint"))]
mod entrypoint {
    use super::*;
    pinocchio::program_entrypoint!(process_instruction, 12);
    pinocchio::no_allocator!();
    pinocchio::nostd_panic_handler!();
}

pub fn process_instruction(
    program_id: &Address,
    accounts: &mut [AccountView],
    data: &[u8],
) -> ProgramResult {
    let Some((tag, args)) = data.split_first_chunk::<8>() else {
        return Err(ProgramError::InvalidInstructionData);
    };

    match *tag {
        MAKE if args.len() >= 24 => {
            let [seed, deposit, receive] = [0, 8, 16]
                .map(|offset| u64::from_le_bytes(args[offset..offset + 8].try_into().unwrap()));
            instructions::make(program_id, accounts, seed, deposit, receive)
        }
        TAKE => instructions::take(program_id, accounts),
        REFUND => instructions::refund(program_id, accounts),
        _ => Err(ProgramError::InvalidInstructionData),
    }
}
