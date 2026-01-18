pipeline {
    agent {
        docker {
            image 'jenkins-agent-rust:jdk21'
            args '''
                -v /var/jenkins/cache/${JOB_NAME}/${BRANCH_NAME}/cargo_registry:/usr/local/cargo/registry
                -v /var/jenkins/cache/${JOB_NAME}/${BRANCH_NAME}/git:/usr/local/cargo/git
                -v /var/jenkins/cache/${JOB_NAME}/${BRANCH_NAME}/target:/app/target
                -e CARGO_TARGET_DIR=/app/target
                -e CARGO_HOME=/usr/local/cargo
                -w /app
            '''
            args '-u root:root'  // 用户权限
            args '-v /var/run/docker.sock:/var/run/docker.sock'  // Docker in Docker
            reuseNode true
            alwaysPull false  // 是否总是拉取最新镜像
        }
    }

    options {
        skipDefaultCheckout(true)
        disableConcurrentBuilds()
        buildDiscarder(logRotator(numToKeepStr: '10'))
        timeout(time: 20, unit: 'MINUTES')
        retry(0)
    }

    parameters {
        choice(
            name: 'BUILD_TYPE',
            choices: ['debug', 'release'],
            description: '选择构建类型'
        )
    }
    environment {
        // 计算缓存路径
        CACHE_BASE = "/var/jenkins/cache/${JOB_NAME}"
        CACHE_DIR = "${CACHE_BASE}/${BRANCH_NAME}"

        PATH = "/home/jenkins/.cargo/bin:${PATH}"   // 设置 Rust 相关环境变量
        RUST_BACKTRACE = '1'                        // 启用完整的错误回溯
        CARGO_INCREMENTAL = '1'                     // 启用增量编译（测试环境）,可能导致无法重现构建
        CARGO_REGISTRIES_CRATES_IO_PROTOCOL = 'sparse'
        RUSTC_WRAPPER = ''                          // 不需要 sccache
        SHOULD_BUILD = 'true'
    }

    stages {

       // 阶段1：分支检查
        stage('Branch Filter') {
            steps {
                script {
                    echo "当前分支: ${BRANCH_NAME}"
                    echo "任务名称: ${JOB_NAME}"

                    // 定义需要构建的分支
                    def allowedBranches = [
                        'main', 'master', 'develop',
                        'release/.*', 'hotfix/.*'
                    ]

                    def shouldRun = allowedBranches.any { pattern ->
                        BRANCH_NAME == pattern || BRANCH_NAME.matches(pattern)
                    }

                    if (shouldRun) {
                        echo "✅ 分支检查通过，开始构建流程"
                        // 默认值即为true
                        // env.SHOULD_BUILD = 'true'
                    } else {
                        currentBuild.result = 'SUCCESS'
                        currentBuild.description = "Skipped: Branch ${BRANCH_NAME} not in build scope"
                        echo "⚠️ 分支 ${BRANCH_NAME} 不在构建范围内，将优雅跳过后续流程"
                        env.SHOULD_BUILD = 'false'
                    }
                }
            }
        }

        stage('Main Workflow') {
            when {
                environment name: 'SHOULD_BUILD', value: 'true'
            }
            stages {
                // 阶段2：缓存初始化
                stage('Initialize Cache') {
            steps {
                sh '''
                    echo "初始化缓存目录: ${CACHE_DIR}"
                    mkdir -p ${CACHE_DIR}/cargo_registry
                    mkdir -p ${CACHE_DIR}/git
                    mkdir -p ${CACHE_DIR}/target

                    # 显示缓存状态
                    echo "缓存目录大小:"
                    du -sh ${CACHE_DIR} 2>/dev/null || echo "新缓存目录"
                '''
            }
        }

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
            when {
                anyOf {
                    branch 'main'
                }
            }
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
            } // end of Main Workflow stages
        } // end of Main Workflow stage
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
            echo "✅ 构建成功: ${JOB_NAME} - ${BRANCH_NAME}"
        }

        failure {
            echo "❌ 构建失败: ${JOB_NAME} - ${BRANCH_NAME}"
            // 失败时输出详细日志
            sh '''
                echo "最后100行构建日志:"
                tail -100 /app/target/build.log 2>/dev/null || echo "无构建日志"
            '''
        }
    }
}