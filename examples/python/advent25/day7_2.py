import os   
from functools import cache

dir = os.path.dirname(__file__)                                            
with open(os.path.join(dir, "day7.txt")) as f:  
    input = f.read()


lines = input.splitlines()

grid = [list(l) for l in lines]

inbounds = lambda x, y: 0 <= x < len(grid[0]) and 0 <= y < len(grid)

s = None
for y, row in enumerate(grid):
    for x, ch in enumerate(row):
        if ch == 'S':
            s = (x, y)
            break
    if s is not None:
        break

@cache
def inner(x: int, y: int) -> int:
    if not inbounds(x, y):
        return 0
    
    if y == len(grid) - 1:
        return 1
    
    if grid[y][x] == '^':
        return inner(x - 1, y) + inner(x + 1, y)
    else:
        return inner(x, y + 1)

out = inner(s[0], s[1])
print(out)
