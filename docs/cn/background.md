---
title: 诞生之因
date: 2024-12-29 09:00:00
author: loongs-zhang
---

# 诞生之因

[English](../en/background.md) | 中文

## 待调优的线程池

在早期程序员为了支持多个用户并发访问服务应用，往往采用多进程方式，即针对每一个TCP网络连接创建一个服务进程。在2000年左右，比较流行使用CGI方式编写Web服务，当时人们用的比较多的Web服务器是基于多进程模式开发的Apache
1.3.x系列，因为进程占用系统资源较多，而线程占用的资源更少，所以人们开始使用多线程的方式(一般都会用到线程池)
编写Web服务应用，这使单台服务器支撑的用户并发度提高了，但依然存在资源浪费的问题。

2020年我入职V公司，由于内部系统不时出现线程池打满的情况，再加上TL读过[《Java线程池实现原理及其在美团业务中的实践》](https://tech.meituan.com/2020/04/02/java-pooling-pratice-in-meituan.html)
，我们决定构建自己的动态线程池，从过程来看，效果不错：

<div style="text-align: center;">
    <img src="/docs/img/begin.jpg" width="50%">
</div>

但是这没有从根本上解决问题。众所周知，线程上下文切换具有一定开销，线程数越多，线程上下文切换开销越大。对于CPU密集型任务，只需保证线程数等于CPU核心数、并将线程绑定到指定CPU核心(
以下简称为`thread-per-core`)
，即可保证最优性能，而对于IO密集型任务，由于任务几乎必定阻塞住线程，线程上下文切换开销一般小于阻塞开销，但当线程数过大时，线程上下文切换开销就会大于阻塞开销了。

动态线程池的本质就是通过调整线程数，尽可能地让线程上下文切换开销小于阻塞开销。由于这个是人工的，那么必然保证不了。

<div style="text-align: center;">
    <img src="/docs/img/run.jpg" width="50%">
</div>

## NIO之痛

那么有没有一种技术能够在保证thread-per-core的前提下，执行IO密集型任务性能不输多线程呢？答案是`NIO`，但仍存在一些限制或者不友好的地方：

1. NIO API使用起来相比BIO API更加复杂；
2. sleep等系统调用会阻塞线程，如果要发挥最佳性能，相当于禁用所有阻塞调用，这对开发者不友好；
3. 在线程池模式下，对于单线程来说，只有当前任务执行完了，才能执行下一个任务，无法实现任务间的公平调度；

PS：假设单线程，CPU时间片为1s，有100个任务，公平调度指每个任务都能公平地占用到10ms的时间片。

1还可以克服，2和3是硬伤，其实如果能够实现3，RPC框架们也不用搞太多线程，只要thread-per-core即可。

如何在能够保证thread-per-core、执行IO密集型任务性能不输多线程的前提下，开发者使用还十分简单呢？`协程`慢慢进入了我的视野。

## Goroutine不香了

一开始玩协程，出于学习成本的考虑，首先选择的是`kotlin`，但当我发现kotlin的协程需要更换API(
比如把Thread.sleep替换为kotlinx.coroutines.delay)才不会阻塞线程后，果断把方向调整为`golang`，大概2周后：

<div style="text-align: center;">
    <img src="/docs/img/good.jpeg" width="50%">
</div>

协程技术哪家强，编程语言找golang。然而随着更深入的学习，我发现几个`goroutine`的不足：

1. `不是thread-per-core`。goroutine运行时也是由线程池来支撑的，而这个线程池的最大线程为256，这个数字一般比thread-per-core的线程数大得多，且调度线程未绑定到CPU；
2. `抢占调度会打断正在运行的系统调用`。如果这个系统调用需要很长时间才能完成，显然会被打断多次，整体性能反而降低；
3. `goroutine离极限性能有明显差距`。对比隔壁c/c++协程库，其性能甚至能到goroutine的1.5倍；

带着遗憾，我开始继续研究c/c++的协程库，发现它们要么是只做了`hook`(
这里解释下hook技术，简单的说，就是代理系统调用，比如调用sleep，没有hook的话会调用操作系统的sleep函数，hook之后会指向我们自己的代码，详细操作步骤可参考`《Linux/Unix系统编程手册》41章和42章`)
，要么只做了`任务窃取`
，还有一些库只提供最基础的`协程抽象`，而最令人失望的是：没有一个协程库实现了`抢占调度`。

没办法，看样子只能自己干了。

<div style="text-align: center;">
    <img src="/docs/img/just_do_it.jpg" width="100%">
</div>
