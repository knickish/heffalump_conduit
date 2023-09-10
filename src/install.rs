use std::ffi::{c_uchar, CString};

use hotsync_conduit_rs::{ConduitInstallation, ConduitManager};

const CREATOR: [c_uchar; 4] = [b'H', b'E', b'F', b'f'];

fn main() {
    let creator = {
        let mut creator = [char::default(); 4];
        for (i, c) in CREATOR.into_iter().enumerate() {
            creator[i] = char::from_u32(c as u32).unwrap();
        }
        creator
    };

    let builder = ConduitInstallation::new_with_creator(
        creator,
        CString::new("heffalump_conduit.dll").unwrap(),
    )
    .unwrap()
    .with_title(CString::new("Heffalump").unwrap());

    ConduitManager::initialize()
        .unwrap()
        .reinstall(builder)
        .unwrap();
}
