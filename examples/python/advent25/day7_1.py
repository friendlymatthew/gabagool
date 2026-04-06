import os
entries = os.listdir('/lib/python3.13')
print('collections' in entries, 're' in      
entries, 'json' in entries)
print(len(entries))  
from collections import deque

dir = os.path.dirname(__file__)                                            
with open(os.path.join(dir, "day7.txt")) as f:  
    input = f.read()

lines = input.splitlines()

grid = [list(l) for l in lines]

s = None
for y, row in enumerate(grid):
    for x, ch in enumerate(row):
        if ch == 'S':
            s = (x, y)
            break
    if s is not None:
        break

print(s)

inbounds = lambda x, y: 0 <= x < len(grid[0]) and 0 <= y < len(grid)

queue = deque([s])
seen_splitters = set()
seen_beams = set()

while queue:
    x, y = queue.popleft()

    if not inbounds(x, y):
        continue
    
    if (x, y) in seen_beams:
        continue
    
    seen_beams.add((x, y))

    if grid[y][x] == '^':
        seen_splitters.add((x, y))

        queue.append((x - 1, y))
        queue.append((x + 1, y))
    else:
        queue.append((x, y + 1))

print(len(seen_splitters)) 