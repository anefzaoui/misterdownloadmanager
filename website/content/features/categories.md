---
name: Categories
title: Download categories by file type
headline: Every file in its folder, without asking.
line: Downloads sorted by type into folders of their own, with Finished and Unfinished one click away.
lede: MDM files each download by its type, music with music and programs with programs, each category with a folder of its own. The pane on the left shows them all, finished or not.
description: Categories in Mister Download Manager: each file type in its own folder, Finished and Unfinished at a glance, and categories of your own.
group: organising
order: 130
since: 0.1.0
shot: /assets/screens/main-light.webp
shot_alt: The main window: the Categories pane on the left with All Downloads, Unfinished, Finished and Queues, and the download list on the right
where: The Categories pane; right-click a category
help: download-list.md#categories
---

## Every file finds its folder

A download falls into a category by its file type: **General**, **Compressed**, **Documents**,
**Music**, **Programs** or **Video**, and torrents into **Torrents**. Each category has its own
folder inside your Downloads folder, so a ZIP file lands in `Downloads\Compressed` and a film in
`Downloads\Video` without you choosing.

Pick another folder in **Download File Info** and, by default, it becomes the category's folder
from then on. **Options → Save to** shows each category's file types and folder in one place.

## Finished and unfinished, at a glance

The pane on the left has four branches. **All Downloads** holds everything, **Unfinished** what
isn't complete yet and **Finished** what is, each by category. **Queues** lists each queue. The
top branches show how many downloads they hold, and right-click → **Browse** opens a category's
folder.

![The Categories pane: All Downloads with a category per file type, Unfinished, Finished and Queues](images/help/list-categories.webp)
*All Downloads, Unfinished, Finished and Queues, each by category.*

MDM opens where you left it, with the same branches open and the same category shown. **View →
Hide categories** gives the list the whole window.

## Make your own

Right-click → **Add category** makes one for what you collect, say **Books** for `EPUB MOBI`:
a name, the file types separated by spaces (`*` allowed) and a folder. Change it later with
**Properties**, or remove it with **Delete**. The files stay where they are.

## The built-in ones are yours too

<!--@coming 0.5-->

**Compressed**, **Documents**, **Music**, **Programs** and **Video** can be renamed, given other
file types or deleted, like your own. A renamed category keeps its folder. Deleting one asks what
happens to its downloads: move them to **General**, or take them off the list. Their files stay
either way.

![A category's properties: its name, file types and folder](images/help/category-properties.webp)
*Right-click a category → Properties: its name, file types and folder.*

Changed your mind? **Options → Save to → Restore the built-in categories** brings them back as
they came, and leaves your own categories alone. **General** and **Torrents** still change only
their folder.
