;; walkthrough for gabagool's time-travel wasm debugger
;; step forward, step back, set breakpoints, and inspect locals/globals/stack as you go

(module
  ;; check the Globals panel, you'll see $counter and $magic throughout
  (global $counter (mut i32) (i32.const 0))
  (global $magic i32 (i32.const 42))

  (memory 1)

  ;; when you land here, look at the Call Stack, you'll see two frames
  (func $square (param $x i32) (result i32)
    ;; Locals shows $x here. watch the Stack as these push values
    local.get $x
    local.get $x
    i32.mul
  )

  ;; computes sum of squares: 1^2 + 2^2 + ... + n^2
  ;; $n defaults to 5 but you can change it in .vscode/launch.json
  (func $demo (param $n i32) (result i32)
    (local $i i32)
    (local $sum i32)
    (local $sq i32)

    ;; you start here! check Locals: $n=5, everything else is 0.
    ;; expand the Stack dropdown, you'll see 1 sitting on top
    i32.const 1
    local.set $i ;; this pops it off the stack and stores it in $i

    i32.const 0
    local.set $sum

    ;; now we enter the loop. try setting a breakpoint somewhere below
    ;; and hitting continue, you'll jump straight there
    block $done
      loop $again
        ;; checking if $i > $n to decide whether to break out
        local.get $i
        local.get $n
        i32.gt_s
        br_if $done

        ;; step INTO here (not over!) to see $square's frame appear
        local.get $i
        call $square
        local.set $sq ;; $sq now holds $i squared

        ;; watch $sum grow as we accumulate: 1, 5, 14, 30, 55
        local.get $sum
        local.get $sq
        i32.add
        local.set $sum

        ;; $counter ticks up each iteration, check Globals
        global.get $counter
        i32.const 1
        i32.add
        global.set $counter

        ;; stashing the running sum in memory at offset $i*4
        ;; open the Memory hex view to watch bytes fill in as you step
        ;; after each i32.store you'll see 4 bytes appear (little-endian):
        ;;   iter 1: 0x04 = 01 00 00 00  ($sum=1)
        ;;   iter 2: 0x08 = 05 00 00 00  ($sum=5)
        ;;   iter 3: 0x0c = 0e 00 00 00  ($sum=14)
        ;;   iter 4: 0x10 = 1e 00 00 00  ($sum=30)
        ;;   iter 5: 0x14 = 37 00 00 00  ($sum=55)
        local.get $i
        i32.const 4
        i32.mul
        local.get $sum
        i32.store

        ;; try stepping BACK from here. $i and $sum rewind to previous values.
        ;; you can step back from anywhere, even across function calls
        local.get $i
        i32.const 1
        i32.add
        local.set $i

        br $again
      end
    end

    ;; all done! $sum should be 55.
    ;; try reverse continue to fly backwards to your breakpoint,
    ;; or step back to walk through the last iteration in reverse
    local.get $sum
  )

  (export "demo" (func $demo))
)
