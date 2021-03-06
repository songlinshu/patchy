use crate::hash::*;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::cmp::min;
use std::collections::{HashMap, HashSet};

pub const DEFAULT_BLOCK_SIZE: usize = 2048;

fn div_up(num: usize, den: usize) -> usize {
    (num + den - 1) / den
}

fn slice_offset_from(slice: &[u8], base: &[u8]) -> u64 {
    slice.as_ptr() as u64 - base.as_ptr() as u64
}

pub struct Block {
    pub offset: u64,
    pub size: u32,
    pub hash_weak: u32,
    pub hash_strong: Hash128,
}

pub fn compute_blocks(input: &[u8], block_size: usize) -> Vec<Block> {
    let chunks = input.chunks(block_size);
    let mut result: Vec<Block> = Vec::with_capacity(chunks.len());
    for chunk in chunks {
        result.push(Block {
            offset: slice_offset_from(chunk, input),
            size: chunk.len() as u32,
            hash_weak: 0,
            hash_strong: Hash128::new_zero(),
        });
    }
    result.par_iter_mut().for_each(|block| {
        let block_begin = block.offset as usize;
        let block_end = block_begin + block.size as usize;
        let block_slice = &input[block_begin..block_end];
        block.hash_weak = compute_hash_weak(block_slice);
        block.hash_strong = compute_hash_strong(block_slice);
    });
    result
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CopyCmd {
    pub source: u64,
    pub target: u64,
    pub size: u32,
}

impl CopyCmd {
    pub fn execute(&self, target: &mut [u8], source: &[u8]) {
        let source_bounds = (
            self.source as usize,
            self.source as usize + self.size as usize,
        );
        let target_bounds = (
            self.target as usize,
            self.target as usize + self.size as usize,
        );
        let source_slice = &source[source_bounds.0..source_bounds.1];
        let target_slice = target[target_bounds.0..target_bounds.1].as_mut();
        target_slice.copy_from_slice(source_slice);
    }
}

pub struct PatchCommands {
    pub base: Vec<CopyCmd>,
    pub other: Vec<CopyCmd>,
}

fn compute_copy_size(cmds: &[CopyCmd]) -> usize {
    let mut result: usize = 0;
    for cmd in cmds {
        result += cmd.size as usize;
    }
    result
}

impl PatchCommands {
    pub fn new() -> Self {
        Self {
            base: Vec::new(),
            other: Vec::new(),
        }
    }
    pub fn need_bytes_from_base(&self) -> usize {
        compute_copy_size(&self.base)
    }
    pub fn need_bytes_from_other(&self) -> usize {
        compute_copy_size(&self.other)
    }
    pub fn is_synchronized(&self) -> bool {
        self.base.is_empty() && self.other.is_empty()
    }
}

fn is_synchronized(sequence: &[Hash128], blocks: &[Block]) -> bool {
    if sequence.len() != blocks.len() {
        return false;
    }
    for it in sequence.iter().zip(blocks.iter()) {
        if *it.0 != it.1.hash_strong {
            return false;
        }
    }
    true
}

pub fn compute_diff(input: &[u8], other_blocks: &[Block], block_size: usize) -> PatchCommands {
    let mut other_block_weak_set: HashSet<u32> = HashSet::new();
    let mut other_block_strong_set: HashSet<Hash128> = HashSet::new();
    let mut base_block_hash_map: HashMap<Hash128, u64> = HashMap::new();
    let mut other_len = 0;
    for block in other_blocks {
        other_block_weak_set.insert(block.hash_weak);
        other_block_strong_set.insert(block.hash_strong);
        other_len += block.size as usize;
    }
    let find_base_block =
        |block_begin: usize, block_end: usize, block_hash_weak: u32| -> Option<Block> {
            if other_block_weak_set.contains(&block_hash_weak) {
                let block_slice = &input[block_begin..block_end];
                let block_hash_strong = compute_hash_strong(block_slice);
                if other_block_strong_set.contains(&block_hash_strong) {
                    let block = Block {
                        offset: block_begin as u64,
                        size: (block_end - block_begin) as u32,
                        hash_weak: block_hash_weak,
                        hash_strong: block_hash_strong,
                    };
                    return Some(block);
                }
            }
            None
        };
    let mut rolling_hash = RollingHash::new();
    let mut window_begin: usize = 0;
    let mut window_end: usize = window_begin;
    let mut sequence: Vec<Hash128> = Vec::new();
    sequence.reserve(div_up(input.len(), block_size));
    loop {
        let remaining_len = input.len() - window_begin;
        if remaining_len == 0 {
            break;
        }
        let this_window_size: usize = min(remaining_len, block_size);
        while rolling_hash.count() < this_window_size {
            rolling_hash.add(input[window_end]);
            window_end += 1;
        }
        match find_base_block(window_begin, window_end, rolling_hash.get()) {
            Some(base_block) => {
                window_begin = window_end;
                rolling_hash = RollingHash::new();
                base_block_hash_map.insert(base_block.hash_strong, base_block.offset);
                sequence.push(base_block.hash_strong);
            }
            None => {
                rolling_hash.sub(input[window_begin]);
                window_begin += 1;
            }
        }
    }
    let mut patch_commands = PatchCommands::new();
    if input.len() != other_len || !is_synchronized(&sequence, &other_blocks) {
        for other_block in other_blocks {
            match base_block_hash_map.get(&other_block.hash_strong) {
                Some(&base_offset) => {
                    patch_commands.base.push(CopyCmd {
                        source: base_offset,
                        target: other_block.offset,
                        size: other_block.size,
                    });
                }
                None => {
                    patch_commands.other.push(CopyCmd {
                        source: other_block.offset,
                        target: other_block.offset,
                        size: other_block.size,
                    });
                }
            }
        }
    }
    patch_commands
}

#[derive(Serialize, Deserialize)]
pub struct Patch {
    pub data: Vec<u8>,
    pub base: Vec<CopyCmd>,
    pub other: Vec<CopyCmd>,
    pub other_size: u64,
}

fn optimize_copy_cmds(cmds: &mut Vec<CopyCmd>) {
    if cmds.len() > 1 {
        cmds.sort_by_key(|v| v.target);
        let (mut prev, rest) = cmds.split_first_mut().unwrap();
        for curr in rest.iter_mut() {
            if prev.source + prev.size as u64 == curr.source
                && prev.target + prev.size as u64 == curr.target
                && prev.size as u64 + curr.size as u64 <= u32::max_value() as u64
            {
                curr.source = prev.source;
                curr.target = prev.target;
                curr.size += prev.size;
                prev.size = 0;
            }
            prev = curr;
        }
        cmds.retain(|cmd| cmd.size != 0);
    }
}

pub fn build_patch(other_data: &[u8], patch_commands: &PatchCommands) -> Patch {
    let mut patch_data: Vec<u8> = Vec::new();
    let mut other_cmds: Vec<CopyCmd> = Vec::new();
    for cmd in &patch_commands.other {
        let patch_copy_cmd = CopyCmd {
            source: patch_data.len() as u64,
            target: cmd.target,
            size: cmd.size,
        };
        let slice_begin = cmd.source as usize;
        let slice_end = cmd.source as usize + cmd.size as usize;
        let slice = &other_data[slice_begin..slice_end];
        patch_data.extend(slice.iter().cloned());
        other_cmds.push(patch_copy_cmd);
    }
    let mut result = Patch {
        data: patch_data,
        base: patch_commands.base.clone(),
        other: other_cmds,
        other_size: other_data.len() as u64,
    };

    optimize_copy_cmds(&mut result.base);
    optimize_copy_cmds(&mut result.other);

    result
}

pub fn apply_patch(base_data: &[u8], patch: &Patch) -> Vec<u8> {
    let mut result: Vec<u8> = Vec::new();
    result.resize(patch.other_size as usize, 0);
    for cmd in &patch.base {
        cmd.execute(&mut result, &base_data);
    }
    for cmd in &patch.other {
        cmd.execute(&mut result, &patch.data);
    }
    result
}

#[cfg(test)]
pub fn testing_optimize_copy_cmds(cmds: &mut Vec<crate::CopyCmd>) {
    optimize_copy_cmds(cmds);
}    
