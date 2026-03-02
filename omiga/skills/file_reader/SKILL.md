---
name: file_reader
description: 读取和总结本地文本文件
emoji: 📄
tags: ["file", "read", "text"]
---

# 文件读取技能

用于读取和总结本地文本文件。

## 支持的文件类型

- 文本文件：.txt, .md, .json, .yaml, .yml, .csv
- 日志文件：.log
- 配置文件：.ini, .toml, .cfg
- 代码文件：.py, .js, .ts, .go, .rs, .java

## 使用方法

```
读取文件 /path/to/file.txt
总结这个文件的内容
```

## 安全注意

- 不会执行任何文件
- 不会读取二进制文件
- 大文件会自动截取部分内容
