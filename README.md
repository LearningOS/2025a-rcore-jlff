# rCore-Tutorial-Code

## Code

- [Soure Code of labs](https://github.com/LearningOS/rCore-Tutorial-Code)

## Documents

- Concise Manual: [rCore-Tutorial-Guide](https://LearningOS.github.io/rCore-Tutorial-Guide/)

- Detail Book [rCore-Tutorial-Book-v3](https://rcore-os.github.io/rCore-Tutorial-Book-v3/)

## OS API docs of rCore Tutorial Code

- [OS API docs of ch1](https://learningos.github.io/rCore-Tutorial-Code/ch1/os/index.html)
  AND [OS API docs of ch2](https://learningos.github.io/rCore-Tutorial-Code/ch2/os/index.html)
- [OS API docs of ch3](https://learningos.github.io/rCore-Tutorial-Code/ch3/os/index.html)
  AND [OS API docs of ch4](https://learningos.github.io/rCore-Tutorial-Code/ch4/os/index.html)
- [OS API docs of ch5](https://learningos.github.io/rCore-Tutorial-Code/ch5/os/index.html)
  AND [OS API docs of ch6](https://learningos.github.io/rCore-Tutorial-Code/ch6/os/index.html)
- [OS API docs of ch7](https://learningos.github.io/rCore-Tutorial-Code/ch7/os/index.html)
  AND [OS API docs of ch8](https://learningos.github.io/rCore-Tutorial-Code/ch8/os/index.html)
- [OS API docs of ch9](https://learningos.github.io/rCore-Tutorial-Code/ch9/os/index.html)

## Related Resources

- [Learning Resource](https://github.com/LearningOS/rust-based-os-comp2025/blob/main/relatedinfo.md)

## Setup

```bash
$ git clone https://github.com/LearningOS/2026s-rcore-[YOUR_USER_NAME].git
$ cd 2026s-rcore-[YOUR_USER_NAME]
```

## Build & Run

```bash
# setup build&run environment first
$ git clone https://github.com/LearningOS/rCore-Tutorial-Test.git user
$ git checkout ch$ID
$ cd os
# run OS in ch$ID
$ make run
```

If you want to use docker to build and run, you can use the following command:
```bash
# After clone the `rCore-Tutorial-Test` repository to your local machine, you can use the following command to build and run:
$ make build_docker
$ make docker
```

If you experience network issues when accessing foreign resources such as GitHub in Docker, you can follow the following suggestions according to your stage:

- Docker pull:
  1. use proxy: https://docs.docker.com/reference/cli/docker/image/pull/#proxy-configuration

  2. use available domestic source (self-search)

- Docker build: use proxy https://docs.docker.com/engine/cli/proxy/#build-with-a-proxy-configuration

- Docker run: use proxy option, related operations are similar to `Docker build`, can refer to the relevant materials by yourself


Notice: $ID is from [1-9]

## Grading

```bash
# setup build&run environment first
$ rm -rf ci-user
$ git clone https://github.com/LearningOS/rCore-Tutorial-Checker.git ci-user
$ git clone https://github.com/LearningOS/rCore-Tutorial-Test.git ci-user/user
$ git checkout ch$ID
# check&grade OS in ch$ID with more tests
$ cd ci-user && make test CHAPTER=$ID
```

Notice: $ID is from [3,4,5,6,8]

## CNB 评测提交指南 / CNB Submission Guide

> 本仓库部署在 [CNB](https://cnb.cool) 平台，评测通过**提 PR** 触发，而非直接 push。
> This repo is on [CNB](https://cnb.cool). Grading is triggered by **opening a Pull Request**, not by pushing directly.

### 步骤 / Steps

**1. Fork 本仓库**

在 CNB 页面点击右上角 **Fork**，将本仓库 fork 到你自己的账号下。

**2. Clone 你的 fork**

```bash
git clone https://cnb.cool/[YOUR_CNB_USERNAME]/rCore-Tutorial-2026S.git
cd rCore-Tutorial-2026S
```

**3. 切换到对应章节分支并完成实验**

```bash
git checkout ch$ID      # $ID 为章节号，如 3、4、5、6、8
# ... 编写代码 ...
git add .
git commit -m "finish ch$ID"
git push origin ch$ID
```

**4. 在 CNB 上提 Pull Request**

进入你 fork 的仓库页面 → 点击 **Pull Requests** → **New Pull Request**：
- **源分支 (Source)**：你的 fork 的 `ch$ID` 分支
- **目标分支 (Target)**：本仓库 `LearningOS/OSCamp-2026S/rCore-Tutorial-2026S` 的 `ch$ID` 分支

提交 PR 后，CI 流水线将**自动运行测试**，满分通过后**自动上传分数**到 OpenCamp。

> 评测章节 / Graded chapters：**ch3、ch4、ch5、ch6、ch8**
> ch1、ch2 无需提交评测。

### 查看评测结果

在 PR 页面可以实时查看 CI 运行日志。测试通过后日志末尾会显示：

```
PASSED: full score X/X
Uploading score for chapter N by user YOUR_USERNAME
```
