(module
  (func $stair_climb (export "stair_climb") (param $n i32) (result i32)
    ;; if (n < 0) return 0
    local.get $n
    i32.const 0
    i32.lt_s
    if (result i32)
      i32.const 0
    else
      ;; if (n == 0) return 1
      local.get $n
      i32.eqz
      if (result i32)
        i32.const 1
      else
        ;; stair_climb(n-1) + stair_climb(n-2) + stair_climb(n-3)
        local.get $n
        i32.const 1
        i32.sub
        call $stair_climb

        local.get $n
        i32.const 2
        i32.sub
        call $stair_climb

        local.get $n
        i32.const 3
        i32.sub
        call $stair_climb

        i32.add
        i32.add
      end
    end
  )
)
