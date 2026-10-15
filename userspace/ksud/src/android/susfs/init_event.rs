use anyhow::{Result, bail};
use std::{
    fs,
    io::{self, Read, Seek},
    os::fd::AsRawFd,
};

use crate::android::{susfs::config::model::Config, utils::daemonize};

fn is_fuse_mounted() -> bool {
    fs::metadata("/sdcard/Android").is_ok()
}

fn wait_for_fuse_mounted() -> Result<()> {
    if is_fuse_mounted() {
        return Ok(());
    }

    let mut file = fs::File::open("/proc/mounts")?;
    let fd = file.as_raw_fd();

    let mut pfd = libc::pollfd {
        fd,
        events: libc::POLLPRI | libc::POLLERR,
        revents: 0,
    };

    log::info!("Polling mount event");

    loop {
        let mut dummy = String::new();
        let _ = file.seek(io::SeekFrom::Start(0));
        let _ = file.read_to_string(&mut dummy);

        if is_fuse_mounted() {
            break;
        }

        let ret = unsafe { libc::poll(&mut pfd, 1, -1) };
        if ret < 0 {
            bail!("Poll failed");
        }
    }

    Ok(())
}

fn handle_result<T>(ret: Result<T>, msg: &str) {
    match ret {
        Ok(_) => log::info!("successfully {msg}!"),
        Err(e) => log::warn!("{msg} failed: {e}!"),
    }
}

pub fn on_boot_completed() {
    log::info!("Start daemonize...");
    if let Err(e) = daemonize(false) {
        log::error!("unable to daemonize: {e}");
        return;
    }
    log::info!("Daemonize success.");

    log::info!("Waiting /sdcard to be mounted.");
    if let Err(e) = wait_for_fuse_mounted() {
        log::error!("Wait FUSE mount failed: {e}");
    }
    log::info!("Processing SUSFS.");

    let config = Config::read_or_default();
    if !config.is_enabled() {
        log::info!("SUSFS persisted configuration is disabled; skipping boot-completed apply");
        return;
    }

    // 以下为原 on_post_fs_data 的内容，挪到这里执行。
    // on_boot_completed 已经 daemonize 并等待 /sdcard FUSE 挂载完成，
    // 此时 Hybrid Mount 的挂载操作早已结束，不会与 SUSFS 抢 post-fs-data 阶段。
    handle_result(config.apply_avc_log_spoofing(), "avc_config");
    handle_result(config.init_sus_kstat(), "initialize sus_kstat");
    handle_result(config.apply_logging(), "sus_log");
    handle_result(
        config.apply_hide_sus_mnts_for_non_su_procs(),
        "hide sus mnts for non su procs",
    );
    handle_result(config.apply_uname(), "sus_uname");

    // 以下为原 on_boot_completed 的内容。
    handle_result(config.apply_cmdline_or_bootconfig(), "sus_cmdline");
    handle_result(config.final_sus_kstat(), "finalize sus_kstat");
    handle_result(config.apply_sus_map(), "sus_map");
    handle_result(config.apply_open_redirect(), "open_redirect");
    handle_result(config.apply_sus_path(), "sus_path and sus_path_loop");

    log::info!("SUSFS finished");
}

pub fn on_post_fs_data() {
    let config = Config::read_or_default();

    log::info!("on_post_fs_data triggered!");

    if !config.is_enabled() {
        log::info!("SUSFS persisted configuration is disabled; skipping post-fs-data apply");
        return;
    }

    // 原 on_post_fs_data 中的所有 apply 操作已全部移至 on_boot_completed。
    // 在此阶段不做任何 SUSFS 下发，避免与 Hybrid Mount 等元模块在 post-fs-data
    // 阶段抢占挂载表，导致 4.14 等老内核上出现随机卡开机 / 无限重启。
    log::info!("SUSFS post-fs-data apply deferred to boot-completed");
}