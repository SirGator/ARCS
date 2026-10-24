use crate::AdapterError;
use arcs_adapter_protocol::{
    AdapterManifest, CapabilityRef, InputDataDescriptor, MAX_DATA_CHUNK_BYTES,
    MAX_INPUT_DATA_BYTES, MAX_INPUT_DATA_OBJECTS, ReceivedInputData,
};
#[derive(Default)]
pub(crate) struct InputDataBuffer {
    batch: Option<Batch>,
}
struct Batch {
    id: String,
    target: CapabilityRef,
    inputs: Vec<ReceivedInputData>,
}
impl InputDataBuffer {
    pub fn clear(&mut self) {
        self.batch = None;
    }
    pub fn begin(
        &mut self,
        manifest: &AdapterManifest,
        id: String,
        target: CapabilityRef,
        inputs: Vec<InputDataDescriptor>,
    ) -> Result<(), AdapterError> {
        if self.batch.is_some()
            || id.trim().is_empty()
            || id.len() > 256
            || target.adapter_id != manifest.adapter_id
            || !manifest
                .capability(&target.capability_id)
                .is_some_and(|cap| cap.contract.is_reasoning())
            || inputs.is_empty()
            || inputs.len() > MAX_INPUT_DATA_OBJECTS
        {
            return Err(AdapterError::rejected("invalid input data batch or target"));
        }
        let mut names = std::collections::HashSet::new();
        let mut total = 0u64;
        for input in &inputs {
            total = total
                .checked_add(input.reference.size_bytes)
                .ok_or_else(|| AdapterError::rejected("input data size overflow"))?;
            if !input.is_valid()
                || !names.insert(&input.selection.name)
                || total > MAX_INPUT_DATA_BYTES
            {
                return Err(AdapterError::rejected(
                    "invalid input data descriptor, duplicate name or size limit",
                ));
            }
        }
        self.batch = Some(Batch {
            id,
            target,
            inputs: inputs
                .into_iter()
                .map(|descriptor| ReceivedInputData {
                    bytes: Vec::new(),
                    descriptor,
                })
                .collect(),
        });
        Ok(())
    }
    pub fn write(
        &mut self,
        id: &str,
        name: &str,
        offset: u64,
        bytes: Vec<u8>,
    ) -> Result<u64, AdapterError> {
        let batch = self
            .batch
            .as_mut()
            .filter(|batch| batch.id == id)
            .ok_or_else(|| AdapterError::rejected("unknown input data batch"))?;
        let input = batch
            .inputs
            .iter_mut()
            .find(|input| input.descriptor.selection.name == name)
            .ok_or_else(|| AdapterError::rejected("unknown input data name"))?;
        if bytes.is_empty()
            || bytes.len() > MAX_DATA_CHUNK_BYTES as usize
            || offset != input.bytes.len() as u64
            || offset + bytes.len() as u64 > input.descriptor.reference.size_bytes
        {
            return Err(AdapterError::rejected("input data offset or size mismatch"));
        }
        input.bytes.extend(bytes);
        Ok(input.bytes.len() as u64)
    }
    pub fn take(
        &mut self,
        id: &str,
        target: &CapabilityRef,
        expected: &[InputDataDescriptor],
    ) -> Result<Vec<ReceivedInputData>, AdapterError> {
        if expected.is_empty() && self.batch.is_none() {
            return Ok(vec![]);
        }
        let batch = self
            .batch
            .take()
            .ok_or_else(|| AdapterError::rejected("input data batch missing"))?;
        if batch.id != id
            || &batch.target != target
            || batch.inputs.len() != expected.len()
            || batch.inputs.iter().zip(expected).any(|(input, expected)| {
                &input.descriptor != expected
                    || input.bytes.len() as u64 != expected.reference.size_bytes
            })
        {
            return Err(AdapterError::rejected(
                "input data incomplete or bound to another invocation",
            ));
        }
        Ok(batch.inputs)
    }
}
