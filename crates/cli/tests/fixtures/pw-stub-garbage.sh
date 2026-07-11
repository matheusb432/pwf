#!/bin/sh
# Allocator stub that always "succeeds" as a process but emits stdout
# `parse_added_id` cannot extract an id from — exercises `handoff add`'s
# orphan-scaffold cleanup when pw allocation fails after the scaffold file is
# already written (PWF-0117 final-review item 4).
printf 'not a task line\n'
