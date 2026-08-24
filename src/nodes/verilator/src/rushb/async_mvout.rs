use super::command::{CommandReceiver, CommandResponse};
use super::dma;
use std::collections::HashMap;
use std::sync::mpsc::TryRecvError;
use std::sync::{Arc, Condvar, Mutex};

struct AsyncMvout {
    host_address: usize,
    status: Mutex<TransferStatus>,
    changed: Condvar,
}

enum TransferStatus {
    Pending(CommandReceiver),
    Waiting,
    Complete(Result<(), String>),
}

static TRANSFERS: Mutex<Option<HashMap<u64, Arc<AsyncMvout>>>> = Mutex::new(None);

pub(crate) fn init() {
    let mut transfers = TRANSFERS.lock().expect("rushB async mvout registry poisoned");
    assert!(transfers.is_none(), "rushB async mvout registry is already initialized");
    *transfers = Some(HashMap::new());
}

pub(crate) fn destroy() {
    let transfers = TRANSFERS
        .lock()
        .expect("rushB async mvout registry poisoned")
        .take()
        .expect("rushB async mvout registry is not initialized");
    assert!(transfers.is_empty(), "rushB async mvout handles remain during shutdown");
}

pub(crate) fn register(handle: u64, host: *mut u8, receiver: CommandReceiver) -> Result<(), String> {
    let transfer = Arc::new(AsyncMvout {
        host_address: host as usize,
        status: Mutex::new(TransferStatus::Pending(receiver)),
        changed: Condvar::new(),
    });
    let mut guard = TRANSFERS
        .lock()
        .map_err(|_| "rushB async mvout registry poisoned".to_string())?;
    let transfers = guard
        .as_mut()
        .ok_or_else(|| "rushB is not initialized; call rushb_init first".to_string())?;
    if transfers.contains_key(&handle) {
        return Err(format!("duplicate rushB async mvout handle {handle}"));
    }
    transfers.insert(handle, transfer);
    Ok(())
}

pub(crate) fn poll(handle: u64) -> Result<bool, String> {
    let transfer = lookup(handle)?;
    let response = {
        let mut status = transfer
            .status
            .lock()
            .map_err(|_| "rushB async mvout status poisoned".to_string())?;
        match &mut *status {
            TransferStatus::Pending(receiver) => match receiver.try_recv() {
                Ok(response) => {
                    *status = TransferStatus::Waiting;
                    Some(response)
                }
                Err(TryRecvError::Empty) => return Ok(false),
                Err(TryRecvError::Disconnected) => {
                    *status = TransferStatus::Waiting;
                    Some(Err(format!(
                        "rushB NPU scheduler stopped while polling async mvout #{handle}"
                    )))
                }
            },
            TransferStatus::Waiting => return Ok(false),
            TransferStatus::Complete(result) => return result.clone().map(|()| true),
        }
    };
    publish(&transfer, response.expect("poll response exists"))?;
    Ok(true)
}

pub(crate) fn wait(handle: u64) -> Result<(), String> {
    let transfer = lookup(handle)?;
    loop {
        let receiver = {
            let mut status = transfer
                .status
                .lock()
                .map_err(|_| "rushB async mvout status poisoned".to_string())?;
            match &*status {
                TransferStatus::Complete(result) => {
                    let result = result.clone();
                    drop(status);
                    remove(handle);
                    return result;
                }
                TransferStatus::Waiting => {
                    drop(
                        transfer
                            .changed
                            .wait(status)
                            .map_err(|_| "rushB async mvout status poisoned".to_string())?,
                    );
                    continue;
                }
                TransferStatus::Pending(_) => {}
            }
            match std::mem::replace(&mut *status, TransferStatus::Waiting) {
                TransferStatus::Pending(receiver) => receiver,
                _ => unreachable!("async mvout status changed while locked"),
            }
        };
        let response = match receiver.recv() {
            Ok(response) => response,
            Err(_) => Err(format!(
                "rushB NPU scheduler stopped while waiting for async mvout #{handle}"
            )),
        };
        let _ = publish(&transfer, response);
    }
}

pub(crate) fn wait_all() -> Result<(), String> {
    let handles = {
        let guard = TRANSFERS
            .lock()
            .map_err(|_| "rushB async mvout registry poisoned".to_string())?;
        guard
            .as_ref()
            .ok_or_else(|| "rushB async mvout registry is not initialized".to_string())?
            .keys()
            .copied()
            .collect::<Vec<_>>()
    };
    for handle in handles {
        wait(handle)?;
    }
    Ok(())
}

fn lookup(handle: u64) -> Result<Arc<AsyncMvout>, String> {
    TRANSFERS
        .lock()
        .map_err(|_| "rushB async mvout registry poisoned".to_string())?
        .as_ref()
        .ok_or_else(|| "rushB async mvout registry is not initialized".to_string())?
        .get(&handle)
        .cloned()
        .ok_or_else(|| format!("unknown rushB async mvout handle {handle}"))
}

fn remove(handle: u64) {
    if let Ok(mut guard) = TRANSFERS.lock() {
        if let Some(transfers) = guard.as_mut() {
            transfers.remove(&handle);
        }
    }
}

fn publish(transfer: &AsyncMvout, response: Result<CommandResponse, String>) -> Result<(), String> {
    let result = response.map(|response| unsafe {
        dma::restore_host(transfer.host_address as *mut u8, &response.output);
    });
    let mut status = transfer
        .status
        .lock()
        .map_err(|_| "rushB async mvout status poisoned".to_string())?;
    *status = TransferStatus::Complete(result.clone());
    transfer.changed.notify_all();
    result
}
