use anyhow::Result;

use crate::examples::{self, ExamplesArgs};

pub fn execute(args: ExamplesArgs) -> Result<()> {
    examples::execute(args)
}
