use pinocchio::{error::ProgramError, Address};

const DISCRIMINATOR: [u8; 8] = [31, 213, 123, 187, 186, 22, 218, 155];

pub struct Escrow {
    pub seed: u64,
    pub maker: Address,
    pub mint_a: Address,
    pub mint_b: Address,
    pub receive: u64,
    pub bump: u8,
}

impl Escrow {
    pub const LEN: usize = 121;

    pub fn unpack(data: &[u8]) -> Result<Self, ProgramError> {
        let data = data
            .first_chunk::<{ Self::LEN }>()
            .ok_or(ProgramError::InvalidAccountData)?;
        if data[..8] != DISCRIMINATOR {
            return Err(ProgramError::InvalidAccountData);
        }
        Ok(Self {
            seed: u64::from_le_bytes(data[8..16].try_into().unwrap()),
            maker: Address::new_from_array(data[16..48].try_into().unwrap()),
            mint_a: Address::new_from_array(data[48..80].try_into().unwrap()),
            mint_b: Address::new_from_array(data[80..112].try_into().unwrap()),
            receive: u64::from_le_bytes(data[112..120].try_into().unwrap()),
            bump: data[120],
        })
    }

    pub(crate) fn pack(&self, data: &mut [u8]) {
        data[..8].copy_from_slice(&DISCRIMINATOR);
        data[8..16].copy_from_slice(&self.seed.to_le_bytes());
        data[16..48].copy_from_slice(self.maker.as_ref());
        data[48..80].copy_from_slice(self.mint_a.as_ref());
        data[80..112].copy_from_slice(self.mint_b.as_ref());
        data[112..120].copy_from_slice(&self.receive.to_le_bytes());
        data[120] = self.bump;
    }
}
