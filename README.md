# Shaderlock - Wayland (wlroots) screenlocker with GPU shaders

## Integrating

A shell script helper to launch Shaderlock as a daemon using systemd is provided
as `shaderlock.daemon`. This lets Shaderlock integrate with lock-signalling
systems like `swayidle`:

```shell
swayidle -w lock shaderlock.daemon before-sleep shaderlock.daemon
```
