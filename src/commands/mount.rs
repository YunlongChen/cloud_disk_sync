use crate::cli::MountCommands;
use crate::mount::fs::CloudFileSystem;
use std::path::Path;

pub fn cmd_mount(command: MountCommands) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        MountCommands::Mount { mountpoint } => {
            println!("Mounting to {}...", mountpoint);

            let fs = CloudFileSystem::new();
            let options = vec![
                fuser::MountOption::RO,
                fuser::MountOption::FSName("cloud-disk-sync".to_string()),
                fuser::MountOption::AutoUnmount,
            ];

            // 检查挂载点是否存在，如果不存在尝试创建
            let path = Path::new(&mountpoint);
            if !path.exists() {
                std::fs::create_dir_all(path)?;
            }

            // 挂载文件系统
            // 注意：这会阻塞当前线程
            fuser::mount2(fs, &mountpoint, &options)?;
        }
        MountCommands::Unmount { mountpoint } => {
            println!("Unmounting {}...", mountpoint);
            // TODO: Implement unmount logic (platform specific)
            // On Linux: fusermount -u <path>
            // On Windows: Typically handled by stopping the FUSE process or via WinFSP tools
            #[cfg(target_os = "linux")]
            {
                std::process::Command::new("fusermount")
                    .arg("-u")
                    .arg(&mountpoint)
                    .status()?;
            }
            #[cfg(not(target_os = "linux"))]
            {
                println!("Unmount not implemented for this OS yet.");
            }
        }
        MountCommands::List {} => {
            println!("Listing mounts not implemented yet.");
        }
    }

    Ok(())
}
