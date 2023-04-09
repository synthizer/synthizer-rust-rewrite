#include "./miniaudio_wrapper.h"

#define MA_API static
#define MINIAUDIO_IMPLEMENTATION

#include "./miniaudio.h"

#include <stdint.h>
#include <stdlib.h>

static ma_log logger;
static struct {
  log_callback_t *error, *warn, *info, *debug;
} log_callbacks;

static ma_context context;

/*
 * Logging callback for Miniaudio.
 */
static void log_callback(void *userdata, ma_uint32 level, const char *message) {
  (void)userdata;

#define LVL(X, Y)                                                                                                      \
  if (level == X && log_callbacks.Y != NULL) {                                                                         \
    log_callbacks.Y(message);                                                                                          \
    return;                                                                                                            \
  }

  LVL(MA_LOG_LEVEL_DEBUG, debug);
  LVL(MA_LOG_LEVEL_INFO, info);
  LVL(MA_LOG_LEVEL_WARNING, warn);
  LVL(MA_LOG_LEVEL_ERROR, error);

  /* If we got here, still at least try. */
  log_callbacks.error(message);
}

uint8_t PREFIX(init_logging)(log_callback_t err_callback, log_callback_t warn_callback, log_callback_t info_callback,
                             log_callback_t debug_callback) {

  log_callbacks.error = err_callback;
  log_callbacks.warn = warn_callback;
  log_callbacks.info = info_callback;
  log_callbacks.debug = debug_callback;

  if (ma_log_init(NULL, &logger) != MA_SUCCESS) {
    return 0;
  }

  ma_log_callback callback = ma_log_callback_init(log_callback, NULL);
  if (ma_log_register_callback(&logger, callback) != MA_SUCCESS) {
    return 0;
  }

  return 1;
}

uint8_t PREFIX(init_context)(void) {
  ma_context_config config = ma_context_config_init();

  config.pLog = &logger;
  if (ma_context_init(NULL, 0, &config, &context) != MA_SUCCESS) {
    return 0;
  }

  return 1;
}

void PREFIX(device_info_deinit)(struct device_info *info) {
  if (info == NULL) {
    return;
  }

  free(info->name);
  free(info->id);
}

uint8_t PREFIX(enumerate_output_devices)(device_enumeration_callback_t *callback, void *userdata) {
  ma_device_info *playback;
  ma_uint32 playback_count;
  ma_device_info *capture;
  ma_uint32 capture_count;
  if (ma_context_get_devices(&context, &playback, &playback_count, &capture, &capture_count) != MA_SUCCESS) {
    return 0;
  }

  for (ma_uint32 i = 0; i < playback_count; i++) {
    char *name;
    ma_device_id *id;

    name = strdup(playback[i].name);
    if (name == NULL) {
      return 1;
    }

    id = malloc(sizeof(*id));
    if (id == NULL) {
      free(name);
      return 1;
    }

    *id = playback[i].id;
    callback(
        &(const struct device_info){
            .name = name,
            .id = id,
            .is_platform_default = playback[i].isDefault,
        },
        userdata);
  }

  return 1;
}

struct wrapped_device {
  ma_device device;
  playback_callback *callback;
  void *rust_userdata;
  struct device_config config;
};

static void data_proc(ma_device *device, void *output, const void *input, ma_uint32 frames) {
  struct wrapped_device *wrapped = device->pUserData;

  wrapped->callback(output, frames * device->playback.channels, &wrapped->config, wrapped->rust_userdata);
}

void *PREFIX(playback_device_open)(const struct device_options *options, playback_callback *cb, void *userdata) {
  ma_device_config config;
  struct wrapped_device *device = NULL;

  config = ma_device_config_init(ma_device_type_playback);
  config.playback.channels = options->channels;
  config.playback.format = ma_format_f32;
  config.sampleRate = options->sr;
  config.dataCallback = data_proc;
  config.playback.pDeviceID = options->device_id;

  device = malloc(sizeof(*device));
  device->rust_userdata = userdata;
  device->callback = cb;
  config.pUserData = device;

  if (ma_device_init(&context, &config, &device->device) != MA_SUCCESS) {
    goto fail;
  }

  device->config.channels = device->device.playback.channels;
  device->config.sr = device->device.sampleRate;

  return device;
fail:
  free(device);
  return NULL;
}

void PREFIX(playback_device_destroy)(void *device_v) {
  struct wrapped_device *device = device_v;

  if (device == NULL) {
    return;
  }

  ma_device_uninit(&device->device);
  free(device);
}

const struct device_config *PREFIX(playback_device_get_config)(void *device_v) {
  struct wrapped_device *device = device_v;
  return &device->config;
}

uint8_t PREFIX(playback_device_stop)(void *device) {
  return ma_device_stop(&((struct wrapped_device *)device)->device) == MA_SUCCESS;
}

uint8_t PREFIX(playback_device_start)(void *device) {
  return ma_device_start(&((struct wrapped_device *)device)->device) == MA_SUCCESS;
}
