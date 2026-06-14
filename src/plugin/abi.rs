//! Host functions imported by plugins under the `"rtdvi"` module name.

use super::runtime::{Caller, Linker};

use super::HostData;
use crate::plugin::pending::PendingAction;

/// Read a UTF-8 string from plugin linear memory.
///
/// Takes `&mut Caller` so that both wasmi (`get_export(&self, …)`) and
/// wasmtime (`get_export(&mut self, …)`) are satisfied at the call sites.
fn read_str(caller: &mut Caller<'_, HostData>, ptr: i32, len: i32) -> Option<String> {
    if len < 0 {
        return None;
    }
    let mem = caller.get_export("memory")?.into_memory()?;
    let mut buf = vec![0u8; len as usize];
    mem.read(&*caller, ptr as usize, &mut buf).ok()?;
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

/// Register all `rtdvi.*` host functions with the given linker.
pub fn register(linker: &mut Linker<HostData>) -> anyhow::Result<()> {
    // ── Logging & status ─────────────────────────────────────────────────────

    linker.func_wrap(
        "rtdvi",
        "rtdvi_log",
        |mut caller: Caller<'_, HostData>, ptr: i32, len: i32| {
            if let Some(msg) = read_str(&mut caller,ptr, len) {
                let name = caller.data().plugin_name.clone();
                tracing::info!("[plugin:{name}] {msg}");
                caller.data_mut().pending.push(PendingAction::Log(msg));
            }
        },
    )?;

    linker.func_wrap(
        "rtdvi",
        "rtdvi_set_status",
        |mut caller: Caller<'_, HostData>, ptr: i32, len: i32| {
            let msg = read_str(&mut caller,ptr, len).unwrap_or_default();
            caller.data_mut().pending.push(PendingAction::SetStatus(msg));
        },
    )?;

    // ── Editor queries (read from snapshot) ──────────────────────────────────

    linker.func_wrap("rtdvi", "rtdvi_active_buffer_id", |caller: Caller<'_, HostData>| -> i32 {
        caller.data().active_buffer_id.map(|id| id as i32).unwrap_or(-1)
    })?;

    linker.func_wrap("rtdvi", "rtdvi_active_window_id", |caller: Caller<'_, HostData>| -> i32 {
        caller.data().active_window_id.map(|id| id as i32).unwrap_or(-1)
    })?;

    linker.func_wrap(
        "rtdvi",
        "rtdvi_line_count",
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
        "rtdvi",
        "rtdvi_get_cursor",
        |caller: Caller<'_, HostData>, win_id: i32| -> i64 {
            match caller.data().cursor_cache.get(&(win_id as u32)) {
                Some(&(row, col)) => ((row as i64) << 32) | (col as i64),
                None => -1,
            }
        },
    )?;

    // ── Buffer line read (writes into plugin-provided buffer) ─────────────────

    linker.func_wrap(
        "rtdvi",
        "rtdvi_get_line",
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
        "rtdvi",
        "rtdvi_get_option_str",
        |mut caller: Caller<'_, HostData>, key_ptr: i32, key_len: i32, out_ptr: i32, max_len: i32| -> i32 {
            let key = match read_str(&mut caller,key_ptr, key_len) {
                Some(k) => k,
                None => return -1,
            };
            let val = caller.data().options.get(&key).cloned().unwrap_or_default();
            write_bytes(&mut caller, out_ptr, max_len, val.as_bytes())
        },
    )?;

    // ── Mutations (deferred via PendingAction) ────────────────────────────────

    linker.func_wrap(
        "rtdvi",
        "rtdvi_insert_text",
        |mut caller: Caller<'_, HostData>, buf_id: i32, char_pos: i32, ptr: i32, len: i32| -> i32 {
            let text = match read_str(&mut caller,ptr, len) {
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
        "rtdvi",
        "rtdvi_delete_text",
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
        "rtdvi",
        "rtdvi_set_cursor",
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
        "rtdvi",
        "rtdvi_register_command",
        |mut caller: Caller<'_, HostData>, ptr: i32, len: i32| -> i32 {
            let name = match read_str(&mut caller, ptr, len) {
                Some(n) => n,
                None => return -1,
            };
            // Fail if the command already exists globally (snapshot) or was
            // already queued in this same rtdvi_init call.
            if caller.data().registered_cmd_names.contains(&name) {
                return -1;
            }
            let already_queued = caller.data().pending.iter().any(|a| {
                matches!(a, PendingAction::RegisterCommand { name: n } if n == &name)
            });
            if already_queued {
                return -1;
            }
            caller.data_mut().pending.push(PendingAction::RegisterCommand { name });
            0
        },
    )?;

    linker.func_wrap(
        "rtdvi",
        "rtdvi_register_plugin_manager",
        |mut caller: Caller<'_, HostData>, ptr: i32, len: i32| -> i32 {
            let ext = match read_str(&mut caller,ptr, len) {
                Some(e) => e,
                None => return -1,
            };
            caller.data_mut().pending.push(PendingAction::RegisterPluginManager { ext });
            0
        },
    )?;

    linker.func_wrap(
        "rtdvi",
        "rtdvi_register_indent_provider",
        |mut caller: Caller<'_, HostData>, ptr: i32, len: i32| -> i32 {
            let filetype = match read_str(&mut caller, ptr, len) {
                Some(ft) => ft,
                None => return -1,
            };
            caller
                .data_mut()
                .pending
                .push(PendingAction::RegisterIndentProvider { filetype });
            0
        },
    )?;

    linker.func_wrap(
        "rtdvi",
        "rtdvi_bind_key",
        |mut caller: Caller<'_, HostData>,
         mode_ptr: i32,
         mode_len: i32,
         keys_ptr: i32,
         keys_len: i32,
         fn_ptr: i32,
         fn_len: i32|
         -> i32 {
            let mode = read_str(&mut caller,mode_ptr, mode_len).unwrap_or_default();
            let keys = read_str(&mut caller,keys_ptr, keys_len).unwrap_or_default();
            let func = read_str(&mut caller,fn_ptr, fn_len).unwrap_or_default();
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

    // WASI random_get: fills the buffer with bytes good enough for HashMap seeding.
    // Plugins must not use this for cryptographic purposes.
    // ── Render buffer (styled, non-editable content) ─────────────────────────
    // Builder API: push spans, break lines, then open. Colours are `0xRRGGBB`
    // (or `-1` = none); `attrs` bits are 1=bold, 2=italic, 4=underline,
    // 8=reverse, 16=strikethrough; `size` is an advisory scale (0=normal,
    // 1..=6=heading). A span with `target_len > 0` is a hyperlink.
    //
    // When the `render-buffer` feature is off the four host functions below are
    // still registered (so plugins that import them instantiate on both
    // runtimes) but as no-ops — `stub_unknown_imports` is not a safe fallback
    // (it traps on wasmtime / fails to resolve on wasmi).
    #[cfg(feature = "render-buffer")]
    {
    linker.func_wrap(
        "rtdvi",
        "rtdvi_render_span",
        |mut caller: Caller<'_, HostData>,
         text_ptr: i32,
         text_len: i32,
         fg: i32,
         bg: i32,
         attrs: i32,
         size: i32,
         target_ptr: i32,
         target_len: i32|
         -> i32 {
            let text = read_str(&mut caller, text_ptr, text_len).unwrap_or_default();
            let link = if target_len > 0 {
                read_str(&mut caller, target_ptr, target_len)
            } else {
                None
            };
            let rgb = |v: i32| -> Option<(u8, u8, u8)> {
                if v < 0 {
                    None
                } else {
                    let v = v as u32;
                    Some((((v >> 16) & 0xff) as u8, ((v >> 8) & 0xff) as u8, (v & 0xff) as u8))
                }
            };
            let spec = super::pending::RenderSpanSpec {
                text,
                fg: rgb(fg),
                bg: rgb(bg),
                bold: attrs & 1 != 0,
                italic: attrs & 2 != 0,
                underline: attrs & 4 != 0,
                reverse: attrs & 8 != 0,
                strike: attrs & 16 != 0,
                size: size.clamp(0, 255) as u8,
                link,
                image: None,
            };
            caller.data_mut().render_current.push(spec);
            0
        },
    )?;

    linker.func_wrap("rtdvi", "rtdvi_render_newline", |mut caller: Caller<'_, HostData>| {
        let line = std::mem::take(&mut caller.data_mut().render_current);
        caller.data_mut().render_lines.push(line);
    })?;

    // Place an image (its own line). Local files are expanded to half-block art
    // at apply time; remote/missing files fall back to a labelled placeholder.
    linker.func_wrap(
        "rtdvi",
        "rtdvi_render_image",
        |mut caller: Caller<'_, HostData>, path_ptr: i32, path_len: i32, alt_ptr: i32, alt_len: i32| -> i32 {
            let path = read_str(&mut caller, path_ptr, path_len).unwrap_or_default();
            let alt = read_str(&mut caller, alt_ptr, alt_len).unwrap_or_default();
            let data = caller.data_mut();
            // Flush any in-progress line first; an image stands on its own line.
            if !data.render_current.is_empty() {
                let line = std::mem::take(&mut data.render_current);
                data.render_lines.push(line);
            }
            data.render_lines.push(vec![super::pending::RenderSpanSpec {
                text: alt,
                image: Some(path),
                ..Default::default()
            }]);
            0
        },
    )?;

    linker.func_wrap(
        "rtdvi",
        "rtdvi_render_open",
        |mut caller: Caller<'_, HostData>, title_ptr: i32, title_len: i32| -> i32 {
            let title = read_str(&mut caller, title_ptr, title_len).unwrap_or_default();
            let data = caller.data_mut();
            // Flush a trailing line built without a final newline.
            if !data.render_current.is_empty() {
                let line = std::mem::take(&mut data.render_current);
                data.render_lines.push(line);
            }
            let lines = std::mem::take(&mut data.render_lines);
            let producer = data.current_command.clone();
            data.pending.push(PendingAction::OpenRenderBuffer { title, producer, lines });
            0
        },
    )?;
    }

    // No-op render ABI when render buffers are compiled out: accept the calls
    // and return success so plugins run, but build nothing.
    #[cfg(not(feature = "render-buffer"))]
    {
        linker.func_wrap(
            "rtdvi",
            "rtdvi_render_span",
            |_: Caller<'_, HostData>, _: i32, _: i32, _: i32, _: i32, _: i32, _: i32, _: i32, _: i32| -> i32 { 0 },
        )?;
        linker.func_wrap("rtdvi", "rtdvi_render_newline", |_: Caller<'_, HostData>| {})?;
        linker.func_wrap(
            "rtdvi",
            "rtdvi_render_image",
            |_: Caller<'_, HostData>, _: i32, _: i32, _: i32, _: i32| -> i32 { 0 },
        )?;
        linker.func_wrap(
            "rtdvi",
            "rtdvi_render_open",
            |_: Caller<'_, HostData>, _: i32, _: i32| -> i32 { 0 },
        )?;
    }

    linker.func_wrap(
        "wasi_snapshot_preview1",
        "random_get",
        |mut caller: Caller<'_, HostData>, buf_ptr: i32, buf_len: i32| -> i32 {
            let mem = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                Some(m) => m,
                None => return 1, // WASI errno EBADF
            };
            let start = buf_ptr as usize;
            let end = start.saturating_add(buf_len as usize);
            let data = mem.data_mut(&mut caller);
            if end > data.len() {
                return 1;
            }
            // Simple deterministic fill: mix index with a constant
            for (i, byte) in data[start..end].iter_mut().enumerate() {
                *byte = (i as u8).wrapping_mul(0x6d).wrapping_add(0x1b);
            }
            0
        },
    )?;

    Ok(())
}
