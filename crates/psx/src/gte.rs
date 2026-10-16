//! GTE (Geometry Transformation Engine), o COP2.
//!
//! Por enquanto só os registradores; os comandos vêm com a GPU.

#[derive(Clone, Default)]
pub(crate) struct Gte {
    data: [u32; 32],
    control: [u32; 32],
}

impl Gte {
    pub(crate) fn read_data(&self, r: usize) -> u32 {
        self.data[r]
    }

    pub(crate) fn write_data(&mut self, r: usize, value: u32) {
        self.data[r] = value;
    }

    pub(crate) fn read_control(&self, r: usize) -> u32 {
        self.control[r]
    }

    pub(crate) fn write_control(&mut self, r: usize, value: u32) {
        self.control[r] = value;
    }

    pub(crate) fn command(&mut self, _instruction: u32) {}
}
