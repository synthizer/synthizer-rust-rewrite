#pragma once

#include <stdint.h>

#define PREFIX(X) syz_miniaudio_0_1_0_##X

typedef void(log_callback_t)(const char *msg);

/*
 * Set up logging by initializing the miniaudio log and associating the specified callbacks for each logging level.
 */
uint8_t PREFIX(init_logging)(log_callback_t err_callback, log_callback_t warn_callback, log_callback_t info_callback,
                             log_callback_t debug_callback);

uint8_t PREFIX(init_context)(void);

struct device_info {
  char *name;
  uint8_t is_platform_default;
  void *id;
};

/*
 * Should copy the struct out to Rust-owned memory.
 */
typedef void(device_enumeration_callback_t)(const struct device_info *, void *);

void PREFIX(device_info_deinit)(struct device_info *info);

/*
 * Enumerate output audio devices, passing them to the callback, which is then mapped to Rust.
 */
uint8_t PREFIX(enumerate_output_devices)(device_enumeration_callback_t *callback, void *userdata);

struct device_options {
  /* If NULL, use platform default. */
  void *device_id;

  unsigned int channels;
  unsigned int sr;
};

struct device_config {
  unsigned int sr;
  unsigned int channels;
};

typedef void(playback_callback)(float *output_buffer, unsigned long long output_buffer_length,
                                const struct device_config *config, void *userdata);

/*
 * Open a device.  The returned `void *` pointer can be used with the other device control functions.
 *
 * The userdata must remain valid until the device is explicitly destroyed.  The returned device is not started.
 */
void *PREFIX(playback_device_open)(const struct device_options *options, playback_callback *cb, void *userdata);
void PREFIX(playback_device_destroy)(void *device);

const struct device_config *PREFIX(playback_device_get_config)(void *device);
uint8_t PREFIX(playback_device_stop)(void *device);
uint8_t PREFIX(playback_device_start)(void *device);
