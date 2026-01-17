pipeline {
    agent {
        docker {
            image 'jenkins-rust-agent:latest'
            label 'rust'  // 这个语法在某些Jenkins版本中可能有效，但并非所有版本都支持
            args '''
                -v rust_cargo_registry:/usr/local/cargo/registry
                -v rust_cargo_git:/usr/local/cargo/git
            '''
            // 或者使用 registryUrl/registryCredentials 如果需要从私有仓库拉取
        }
    }

    environment {
        // 设置 Rust 相关环境变量
        PATH = "/home/jenkins/.cargo/bin:${PATH}"
        CARGO_INCREMENTAL = '0'  // 禁用增量编译以获得可重现的构建
        RUST_BACKTRACE = '1'     // 启用完整的错误回溯
    }

    options {
        buildDiscarder(logRotator(numToKeepStr: '10'))
        timeout(time: 30, unit: 'MINUTES')
        retry(2)
    }

    stages {
        stage('Checkout') {
            steps {
                checkout scm
                sh 'git submodule update --init --recursive'  // 如果使用子模块
            }
        }

        stage('Setup Rust') {
            steps {
                script {
                    // 检查并更新 Rust 工具链
                    sh '''
                        echo "=== Rust Toolchain Info ==="
                        // apt-get update && apt-get install -y curl
                        // curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --profile default
                        rustc --version
                        cargo --version
                        # 更新到最新稳定版（可选）
                        #rustup update stable

                        # 如果需要特定版本
                        # rustup default 1.70.0
                    '''
                }
            }
        }

        stage('Build') {
            steps {
                sh '''
                    echo "=== Starting Build ==="
                    cargo build --verbose

                    # 如果是发布构建
                    # cargo build --release --verbose
                '''
            }
        }

        stage('Test') {
            steps {
                sh '''
                    echo "=== Running Tests ==="
                    cargo test --verbose

                    # 只运行单元测试
                    # cargo test --lib --verbose

                    # 生成测试覆盖率报告（需要安装 tarpaulin）
                    # cargo tarpaulin --verbose --out Xml
                '''
            }
        }

        stage('Clippy & Format Check') {
            steps {
                sh '''
                    echo "=== Running Lints ==="
                    # 安装 clippy 和 rustfmt 如果还没有
                    rustup component add clippy rustfmt

                    cargo clippy -- -D warnings
                    cargo fmt -- --check
                '''
            }
        }

        stage('Generate Docs') {
            steps {
                sh '''
                    echo "=== Generating Documentation ==="
                    cargo doc --no-deps

                    # 如果文档需要对外开放
                    # mkdir -p target/doc
                    # cp -r target/doc/* /var/jenkins/docs/ 2>/dev/null || true
                '''
            }
        }
    }

    post {
        always {
            // 清理构建缓存以节省空间
            sh '''
                echo "=== Cleaning Up ==="
                cargo clean -p ${JOB_NAME} 2>/dev/null || true

                # 显示构建产物大小
                du -sh target/ 2>/dev/null || true
            '''

            // 存档重要文件
            archiveArtifacts artifacts: 'target/release/**/*', fingerprint: true, onlyIfSuccessful: false
            junit 'target/**/test-results/*.xml'  // 如果使用类似 cargo-test-junit 的插件
        }

        success {
            echo '✅ Build successful!'
            // 可以在这里添加 Slack、邮件通知等
        }

        failure {
            echo '❌ Build failed!'
            // 失败通知
        }

        unstable {
            echo '⚠️ Build unstable!'
        }
    }
}