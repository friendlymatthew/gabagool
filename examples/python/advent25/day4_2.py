import os   

dir = os.path.dirname(__file__)                                            
with open(os.path.join(dir, "day4.txt")) as f:  
    input = f.read()

DIR = [
    (-1, 0),
    (0, -1),
    (0, 1),
    (1, 0),
    (1, 1),
    (1, -1),
    (-1, 1),
    (-1, -1)
]

lines = input.splitlines()

inbounds = lambda x, y: 0 <= x < len(lines[0]) and 0 <= y < len(lines)

grid = [list(line) for line in lines]

start = []
for y, line in enumerate(grid):
    for x, ch in enumerate(line):

        if ch == '@':
            start.append((x, y))

removed = 0
stack = start

while stack:
    x, y = stack.pop()
    
    if grid[y][x] != '@':
        continue
    
    out = 0
    for dx, dy in DIR:
        nx, ny = x + dx, y + dy
        if not inbounds(nx, ny):
            continue

        out += int(grid[ny][nx] == '@')

    if out < 4:
        removed += 1
        grid[y][x] = '.'

        for dx, dy in DIR:
            nx, ny = x + dx, y + dy

            if inbounds(nx, ny) and grid[ny][nx] == '@':
                stack.append((nx, ny))


print(removed)