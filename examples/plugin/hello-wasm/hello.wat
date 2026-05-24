;; hello-wasm — minimal jvim plugin in raw WebAssembly Text format.
;;
;; Build:
;;   wat2wasm hello.wat -o hello.wasm
;;   cp hello.wasm ~/.local/jvim/plugins/
;;
;; Enable in ~/.config/jvim/config.toml:
;;   plugins = ["hello"]
;;
;; Commands:
;;   :plugin hello.greet()   — sets status to "Hello from WebAssembly!"
;;   <leader>h               — same (bound in jvim_init)

(module
  ;; ── Host imports (module "jvim") ─────────────────────────────────────────

  (import "jvim" "jvim_log"              (func $jvim_log        (param i32 i32)))
  (import "jvim" "jvim_set_status"       (func $jvim_set_status (param i32 i32)))
  (import "jvim" "jvim_register_command" (func $jvim_register_command (param i32 i32) (result i32)))
  (import "jvim" "jvim_bind_key"         (func $jvim_bind_key
    (param i32 i32 i32 i32 i32 i32) (result i32)))

  ;; ── Linear memory (1 page = 64 KiB) ─────────────────────────────────────

  (memory (export "memory") 1)

  ;; ── Static string data ───────────────────────────────────────────────────

  (data (i32.const 0)  "hello-wasm: loaded")          ;; offset  0, len 18
  (data (i32.const 20) "Hello from WebAssembly!")      ;; offset 20, len 23
  (data (i32.const 50) "greet")                        ;; offset 50, len  5
  (data (i32.const 60) "normal")                       ;; offset 60, len  6
  (data (i32.const 70) "<leader>h")                    ;; offset 70, len  9
  (data (i32.const 82) "hello.greet")                  ;; offset 82, len 11
  (data (i32.const 96) "unknown command")               ;; offset 96, len 15

  ;; ── Memory exports required by the host ──────────────────────────────────

  ;; The host never actually allocates into this plugin's memory (all strings
  ;; passed in are read-only slices the host owns), but alloc/dealloc must be
  ;; exported to satisfy the host ABI.

  (func (export "alloc") (param $size i32) (result i32)
    ;; Bump allocator: keep a pointer at offset 512, grow upward.
    (local $ptr i32)
    (local.set $ptr (i32.load (i32.const 508)))
    (if (i32.eqz (local.get $ptr))
      (then (local.set $ptr (i32.const 512)))
    )
    (i32.store (i32.const 508) (i32.add (local.get $ptr) (local.get $size)))
    (local.get $ptr)
  )

  (func (export "dealloc") (param $ptr i32) (param $size i32)
    ;; No-op: bump allocator does not free individual allocations.
    (return)
  )

  ;; ── jvim_init — called once when the plugin loads ────────────────────────

  (func (export "jvim_init") (param $cfg_ptr i32) (param $cfg_len i32) (result i32)
    ;; Log "hello-wasm: loaded"
    (call $jvim_log (i32.const 0) (i32.const 18))

    ;; Register the "greet" command
    (drop (call $jvim_register_command (i32.const 50) (i32.const 5)))

    ;; Bind <leader>h → hello.greet  (mode="normal", keys="<leader>h", fn="hello.greet")
    (drop (call $jvim_bind_key
      (i32.const 60) (i32.const 6)   ;; mode
      (i32.const 70) (i32.const 9)   ;; keys
      (i32.const 82) (i32.const 11)  ;; fn
    ))

    (i32.const 0)  ;; 0 = accept plugin
  )

  ;; ── run_command — dispatched for every registered command ────────────────

  (func (export "run_command")
    (param $name_ptr i32) (param $name_len i32)
    (param $args_ptr i32) (param $args_len i32)
    (result i32)

    ;; Check name == "greet" (5 bytes at offset 50)
    (if (i32.and
          (i32.eq (local.get $name_len) (i32.const 5))
          (i32.eq
            (i32.load (local.get $name_ptr))
            (i32.load (i32.const 50))))
      (then
        (call $jvim_set_status (i32.const 20) (i32.const 23))
        (return (i32.const 0))
      )
    )

    ;; Unknown command
    (call $jvim_log (i32.const 96) (i32.const 15))
    (i32.const -1)
  )
)
