#pragma once
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

// Unified entry point across macOS/iOS/Android.
// Arguments are UTF-8 strings.
int rfvp_run_entry(const char* game_root_utf8, const char* nls_utf8);

// Desktop options schema v1, UTF-8 JSON, at most 65536 bytes (no trailing NUL).
// Example: {"plugins":[],"sakura_moyu_chs_patch":true}
// Unknown/duplicate fields are errors; {} preserves legacy plugin-disabled behavior.
// External packages require the external-plugins build feature.
// The bundled alias requires sakura-moyu unless an explicit Sakura package is selected.
// Experimental: save/profile compatibility enforcement is not implemented yet.
// Returns 0 on success, 1 on launch failure, 2 on invalid arguments/options.
int rfvp_run_entry_v1(const char* game_root_utf8, const char* nls_utf8,
                      const unsigned char* options_json, size_t options_len);

// Sets the default for instances created after this call.
void rfvp_set_text_hidpi_enabled(int enabled);

// Pump-mode entry points (for embedding into an existing UI runloop).
// Returns an opaque handle, or NULL on failure.
void* rfvp_pump_create(const char* game_root_utf8, const char* nls_utf8);
void* rfvp_pump_create_v1(const char* game_root_utf8, const char* nls_utf8,
                          const unsigned char* options_json, size_t options_len);

// Steps the pump. timeout_ms should be 0 for non-blocking usage on the UI thread.
// Returns 0 to continue, non-zero to stop (exited / fatal error).
int rfvp_pump_step(void* handle, unsigned int timeout_ms);

// enabled: 0 disables HiDPI text backing, non-zero enables it.
void rfvp_pump_set_text_hidpi(void* handle, int enabled);

// Destroys the pump handle.
void rfvp_pump_destroy(void* handle);

#ifdef __cplusplus
} // extern "C"
#endif
