---
name: Scheduler and queues
title: Download scheduler and queues
headline: Queue it today. Download it tonight.
line: Queues started and stopped by the clock, a few files at a time, the computer off at the end.
lede: Put downloads in a queue and let the Scheduler start them when nobody needs the connection. When the queue is done, MDM can quit or turn the computer off.
description: Mister Download Manager's queues and Scheduler: start downloads at night, stop them in the morning, stay under a data cap, turn the computer off.
group: organising
order: 110
since: 0.1.0
shot: /assets/screens/scheduler-light.webp
shot_alt: The Scheduler: the queues on the left, the Main download queue's Schedule tab on the right with Start download at, Daily, Stop download at and Turn off computer when done
where: Downloads → Scheduler
help: queues-and-scheduler.md
---

## Downloads that wait their turn

A queue is a list of downloads that run in order, a few at a time. Click **Download Later** in
**Download File Info**, or right-click downloads in the list and choose **Add to queue**. The
**Q** column marks them, and **Queues** in the categories pane shows each queue on its own.

A queue runs four downloads at once by default. Set another number on its **Files in the
queue** tab, or 1 to fetch them one by one. A download that fails moves to the end and is tried
again 30 seconds later. Besides the **Main download queue**, you can make as many queues as you
like.

## Started and stopped by the clock

On a queue's **Schedule** tab, tick **Start download at** and choose a time, **Once at** a date
or **Daily** on the days you tick. **Stop download at** halts whatever is left, for example
before work in the morning. A queue can also start whenever MDM starts.

MDM has to be running for a schedule to fire. Closing the main window is fine: MDM keeps going
in the notification area.

## When the queue is done

MDM can open a file, such as a sound or a script, exit, or turn the computer off with **Shut
down**, **Sleep** or **Hibernate**. A 30-second countdown comes first, so if you are still at the
computer, **Cancel** stops it.

![The Scheduler's choices for when the queue is done, Turn off computer when done among them](images/help/overnight-done.webp)
*When the queue is done: open a file, quit MDM, or turn the computer off.*

## Woken up for the queue

<!--@coming 0.5-->

Tick **Wake the computer to run this queue** and a sleeping computer is woken a minute before
the start time, so the network is back when the queue begins. It stays awake while the queue
runs; afterwards its own sleep settings apply again. The line under the option says when the
next wake is, or why there can't be one.

The computer has to be asleep, not shut down. On Windows the power plan must allow wake timers:
the Scheduler says when it doesn't, and **Change** opens the setting. On Linux an administrator
allows it once. On macOS only an administrator can schedule a wake, so the option is greyed
there. MDM never asks for an administrator's password.

## Under a data cap

**Download limits** keeps a capped internet plan in check: no more than a set number of
megabytes every few hours. When the limit is reached, downloads wait, and they carry on by
themselves when the period is over.

[Download overnight](guide-download-overnight.md) walks through it step by step.
