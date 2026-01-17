pipeline {
    agent {
        docker {
            image 'jenkins-rust-agent:latest'
            label 'rust'  // 这个语法在某些Jenkins版本中可能有效，但并非所有版本都支持
            args '''
                -v rust-cargo-registry:/usr/local/cargo/registry
                -v rust-cargo-git:/usr/local/cargo/git
                -v rust-target:/home/jenkins/agent/target
                -e CARGO_TARGET_DIR=/home/jenkins/agent/target
                -e CARGO_HOME=/usr/local/cargo
            '''
            // 或者使用 registryUrl/registryCredentials 如果需要从私有仓库拉取
        }
    }

    environment {
        PATH = "/home/jenkins/.cargo/bin:${PATH}"   // 设置 Rust 相关环境变量
        RUST_BACKTRACE = '1'                        // 启用完整的错误回溯
        CARGO_INCREMENTAL = '1'                     // 启用增量编译（测试环境）,可能导致无法重现构建
        CARGO_REGISTRIES_CRATES_IO_PROTOCOL = 'sparse'
        RUSTC_WRAPPER = ''                          // 不需要 sccache
    }

    options {
        buildDiscarder(logRotator(numToKeepStr: '10'))
        timeout(time: 30, unit: 'MINUTES')
        retry(0)
    }

    stages {
        stage('Checkout') {
            steps {
                checkout scm
                sh 'git submodule update --init --recursive'  // 如果使用子模块
            }
        }

        // 第一阶段：准备依赖（可并行）
        stage('Prepare') {
            parallel {
                stage('Fetch Dependencies') {
                    steps {
                        sh 'time cargo fetch --locked'
                    }
                }
                stage('Update Toolchain') {
                    steps {
                        sh '''
                            rustc --version
                            cargo --version
                            rustup update --no-self-update
                            rustup component add clippy rustfmt
                        '''
                    }
                }
            }
        }
        stage('Build') {
            steps {
                sh '''
                    echo "=== Starting Build ==="
                    # 启用链接时间优化（LTO）
                    time cargo build --locked --frozen

                    # 如果是发布构建
                    # time cargo build --release --locked --frozen
                '''
            }
        }

        stage('Test') {
            steps {
                sh '''
                    echo "=== Running Tests ==="
                    time cargo test --locked --no-fail-fast --jobs $(nproc)

                    # 只运行单元测试
                    # cargo test --lib 

                    # 生成测试覆盖率报告（需要安装 tarpaulin）
                    # cargo tarpaulin --out Xml
                '''
            }
        }

        stage('Clippy & Format Check') {
            steps {
                    script {
                        // 只有特定分支或标签才执行耗时操作
                        if (env.BRANCH_NAME == 'main' || env.BRANCH_NAME.startsWith('release/')) {
                            sh '''
                                echo "=== Running full checks on main/release ==="
                                cargo clippy -- -D warnings
                                cargo fmt -- --check
                                cargo doc --no-deps
                            '''
                        } else {
                            sh '''
                                echo "=== Running minimal checks on feature branch ==="
                                cargo clippy -- -D warnings || true  # 不阻塞
                            '''
                        }
                    }
                }
        }

        stage('Generate Docs') {
            steps {
                sh '''
                    echo "=== Generating Documentation ==="
                    # time cargo doc --no-deps

                    # 如果文档需要对外开放
                    # mkdir -p target/doc
                    # cp -r target/doc/* /var/jenkins/docs/ 2>/dev/null || true
                '''
            }
        }
    }

    post {
        always {
            script {
                def duration = currentBuild.duration
                println "构建耗时: ${duration/1000} 秒"
                // 可以推送到监控系统
            }
            // 清理，但保留依赖缓存
            sh '''
                echo "=== Cleaning intermediate files ==="
                # 只清理中间文件，保留依赖
                find target -name "*.d" -delete 2>/dev/null || true
                find target -name "*.o" -delete 2>/dev/null || true
                find target -name "*.rlib" -delete 2>/dev/null || true

                # 显示最终磁盘使用
                du -sh target/ 2>/dev/null || true
                du -sh /usr/local/cargo/registry/ 2>/dev/null || true
            '''

            // 只存档重要产物
            // archiveArtifacts artifacts: 'target/release/cloud-disk-sync*', fingerprint: true


        }

        success {
            echo "✅ Build succeeded in ${currentBuild.durationString}"
        }

        failure {
            echo "❌ Build failed"
            // 失败时输出详细日志
            sh 'cargo build --verbose 2>&1 | tail -100'
        }
    }
}