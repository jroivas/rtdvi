//! Host functions imported by plugins under the `"jvim"` module name.

use wasmi::{Caller, Linker};

use super::HostData;
use crate::plugin::pending::PendingAction;

/// Read a UTF-8 string from plugin linear memory.
fn read_str(caller: &Caller<'_, HostData>, ptr: i32, len: i32) -> Option<String> {
    if len < 0 {
        return None;
    }
    let mem = caller.get_export("memory")?.into_memory()?;
    let mut buf = vec![0u8; len as usize];
    mem.read(caller, ptr as usize, &mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// Write bytes to plugin memory at `ptr`. Returns bytes written or -1 on error.
fn write_bytes(caller: &mut Caller<'_, HostData>, ptr: i32, max_len: i32, data: &[u8]) -> i32 {
    let mem = match caller.get_export("memory").and_then(|e| e.into_memory()) {
        Some(m) => m,
        None => return -1,
    };
    let write_len = data.len().min(max_len as usize);
    if mem.write(caller, ptr as usize, &data[..write_len]).is_err() {
        return -1;
    }
    write_len as i32
}

/// Register all `jvim.*` host functions with the given linker.
pub fn register(linker: &mut Linker<HostData>) -> Result<(), wasmi::Error> {
    // ── Logging & status ─────────────────────────────────────────────────────

    linker.func_wrap(
        "jvim",
        "jvim_log",
        |mut caller: Caller<'_, HostData>, ptr: i32, len: i32| {
            if let Some(msg) = read_str(&caller, ptr, len) {
                let name = caller.data().plugin_name.clone();
                tracing::info!("[plugin:{name}] {msg}");
                caller.data_mut().pending.push(PendingAction::Log(msg));
            }
        },
    )?;

    linker.func_wrap(
        "jvim",
        "jvim_set_status",
        |mut caller: Caller<'_, HostData>, ptr: i32, len: i32| {
            let msg = read_str(&caller, ptr, len).unwrap_or_default();
            caller.data_mut().pending.push(PendingAction::SetStatus(msg));
        },
    )?;

    // ── Editor queries (read from snapshot) ──────────────────────────────────

    linker.func_wrap("jvim", "jvim_active_buffer_id", |caller: Caller<'_, HostData>| -> i32 {
        caller.data().active_buffer_id.map(|id| id as i32).unwrap_or(-1)
    })?;

    linker.func_wrap("jvim", "jvim_active_window_id", |caller: Caller<'_, HostData>| -> i32 {
        caller.data().active_window_id.map(|id| id as i32).unwrap_or(-1)
    })?;

    linker.func_wrap(
        "jvim",
        "jvim_line_count",
        |caller: Caller<'_, HostData>, buf_id: i32| -> i32 {
            caller
                .data()
                .line_count_cache
                .get(&(buf_id as u32))
                .copied()
                .map(|n| n as i32)
                .unwrap_or(-1)
        },
    )?;

    linker.func_wrap(
        "jvim",
        "jvim_get_cursor",
        |caller: Caller<'_, HostData>, win_id: i32| -> i64 {
            match caller.data().cursor_cache.get(&(win_id as u32)) {
                Some(&(row, col)) => ((row as i64) << 32) | (col as i64),
                None => -1,
            }
        },
    )?;

    // ── Buffer line read (writes into plugin-provided buffer) ─────────────────

    linker.func_wrap(
        "jvim",
        "jvim_get_line",
        |mut caller: Caller<'_, HostData>, buf_id: i32, row: i32, out_ptr: i32, max_len: i32| -> i32 {
            let line = {
                let data = caller.data();
                data.line_cache
                    .get(&(buf_id as u32))
                    .and_then(|lines| lines.get(row as usize))
                    .cloned()
                    .unwrap_or_default()
            };
            write_bytes(&mut caller, out_ptr, max_len, line.as_bytes())
        },
    )?;

    // ── Editor option read ────────────────────────────────────────────────────

    linker.func_wrap(
        "jvim",
        "jvim_get_option_str",
        |mut caller: Caller<'_, HostData>, key_ptr: i32, key_len: i32, out_ptr: i32, max_len: i32| -> i32 {
            let key = match read_str(&caller, key_ptr, key_len) {
                Some(k) => k,
                None => return -1,
            };
            let val = caller.data().options.get(&key).cloned().unwrap_or_default();
            write_bytes(&mut caller, out_ptr, max_len, val.as_bytes())
        },
    )?;

    // ── Mutations (deferred via PendingAction) ────────────────────────────────

    linker.func_wrap(
        "jvim",
        "jvim_insert_text",
        |mut caller: Caller<'_, HostData>, buf_id: i32, char_pos: i32, ptr: i32, len: i32| -> i32 {
            let text = match read_str(&caller, ptr, len) {
                Some(t) => t,
                None => return -1,
            };
            caller.data_mut().pending.push(PendingAction::InsertText {
                buffer_id: buf_id as u32,
                char_pos: char_pos as usize,
                text,
            });
            0
        },
    )?;

    linker.func_wrap(
        "jvim",
        "jvim_delete_text",
        |mut caller: Caller<'_, HostData>, buf_id: i32, start: i32, end: i32| -> i32 {
            caller.data_mut().pending.push(PendingAction::DeleteText {
                buffer_id: buf_id as u32,
                start: start as usize,
                end: end as usize,
            });
            0
        },
    )?;

    linker.func_wrap(
        "jvim",
        "jvim_set_cursor",
        |mut caller: Caller<'_, HostData>, win_id: i32, row: i32, col: i32| -> i32 {
            caller.data_mut().pending.push(PendingAction::SetCursor {
                window_id: win_id as u32,
                row: row as usize,
                col: col as usize,
            });
            0
        },
    )?;

    linker.func_wrap(
        "jvim",
        "jvim_register_command",
        |mut caller: Caller<'_, HostData>, ptr: i32, len: i32| -> i32 {
            let name = match read_str(&caller, ptr, len) {
                Some(n) => n,
                None => return -1,
            };
            caller.data_mut().pending.push(PendingAction::RegisterCommand { name });
            0
        },
    )?;

    linker.func_wrap(
        "jvim",
        "jvim_register_plugin_manager",
        |mut caller: Caller<'_, HostData>, ptr: i32, len: i32| -> i32 {
            let ext = match read_str(&caller, ptr, len) {
                Some(e) => e,
                None => return -1,
            };
            caller.data_mut().pending.push(PendingAction::RegisterPluginManager { ext });
            0
        },
    )?;

    linker.func_wrap(
        "jvim",
        "jvim_bind_key",
        |mut caller: Caller<'_, HostData>,
         mode_ptr: i32,
         mode_len: i32,
         keys_ptr: i32,
         keys_len: i32,
         fn_ptr: i32,
         fn_len: i32|
         -> i32 {
            let mode = read_str(&caller, mode_ptr, mode_len).unwrap_or_default();
            let keys = read_str(&caller, keys_ptr, keys_len).unwrap_or_default();
            let func = read_str(&caller, fn_ptr, fn_len).unwrap_or_default();
            // func is "pluginname.functionname"
            let (pname, fname) = match func.split_once('.') {
                Some((p, f)) => (p.to_string(), f.to_string()),
                None => {
                    return -1;
                }
            };
            caller.data_mut().pending.push(PendingAction::BindKey {
                mode,
                keys,
                plugin_name: pname,
                function: fname,
            });
            0
        },
    )?;

    Ok(())
}
