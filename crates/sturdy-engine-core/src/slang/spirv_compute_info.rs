/// Extract compute shader properties from SPIR-V bytecode.
///
/// Scans the SPIR-V instruction stream for `OpCapability` (wave/subgroup ops)
/// and `OpExecutionMode LocalSize` (declared workgroup dimensions). Both are
/// available in any valid SPIR-V output from Slang.

const OP_CAPABILITY: u16 = 17;
const OP_EXECUTION_MODE: u16 = 16;

const EXECUTION_MODE_LOCAL_SIZE: u32 = 17;

/// Properties extracted from SPIR-V about compute/mesh/task shader capabilities.
pub struct SpirvComputeInfo {
    /// Declared workgroup dimensions from `OpExecutionMode LocalSize`.
    /// `[1, 1, 1]` for graphics-only stages or when the mode is absent.
    pub workgroup_size: [u32; 3],
    /// True when any subgroup/wave-operation capability is declared.
    /// Subgroup size must be respected at pipeline creation when this is true.
    pub wave_ops_used: bool,
}

/// Scan `words` for compute-relevant SPIR-V declarations.
///
/// Safe to call on any SPIR-V binary (including vertex/fragment); properties
/// that are not relevant to the shader's stage simply keep their default values.
pub fn reflect_spirv_compute_info(words: &[u32]) -> SpirvComputeInfo {
    if words.len() < 5 || words[0] != 0x0723_0203 {
        return SpirvComputeInfo {
            workgroup_size: [1, 1, 1],
            wave_ops_used: false,
        };
    }

    let mut workgroup_size = [1u32, 1, 1];
    let mut wave_ops_used = false;

    let mut index = 5usize;
    while index < words.len() {
        let first_word = words[index];
        let word_count = (first_word >> 16) as usize;
        let opcode = (first_word & 0xffff) as u16;
        if word_count == 0 || index + word_count > words.len() {
            break;
        }
        let instr = &words[index..index + word_count];
        index += word_count;

        match opcode {
            OP_CAPABILITY if word_count >= 2 => {
                if is_wave_op_capability(instr[1]) {
                    wave_ops_used = true;
                }
            }
            OP_EXECUTION_MODE
                if word_count >= 6 && instr[2] == EXECUTION_MODE_LOCAL_SIZE =>
            {
                workgroup_size = [instr[3], instr[4], instr[5]];
            }
            _ => {}
        }
    }

    SpirvComputeInfo {
        workgroup_size,
        wave_ops_used,
    }
}

/// Returns true when `cap` is a SPIR-V capability that implies wave/subgroup intrinsics.
fn is_wave_op_capability(cap: u32) -> bool {
    matches!(
        cap,
        18   // Groups (older wave broadcast/reduce)
        | 61 // GroupNonUniform
        | 62 // GroupNonUniformVote
        | 63 // GroupNonUniformArithmetic
        | 64 // GroupNonUniformBallot
        | 65 // GroupNonUniformShuffle
        | 66 // GroupNonUniformShuffleRelative
        | 67 // GroupNonUniformClustered
        | 68 // GroupNonUniformQuad
        | 4423 // SubgroupBallotKHR
        | 4427 // SubgroupVoteKHR
        | 5297 // GroupNonUniformPartitionedNV
    )
}
