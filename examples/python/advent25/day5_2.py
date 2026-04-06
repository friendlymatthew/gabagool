import os   
import bisect

dir = os.path.dirname(__file__)                                            
with open(os.path.join(dir, "day5.txt")) as f:  
    input = f.read()

lines = input.splitlines()
parse_ingredients = False
fresh_list = []
ingredients = []

for line in lines:
    if len(line) == 0:
        parse_ingredients = True 
        continue 

    if parse_ingredients:
        ingredients.append(int(line))
        continue

    start, end = line.split('-')
    fresh_list.append((int(start), int(end)))


fresh_list = sorted(fresh_list)

merged = [fresh_list[0]]

for start, end in fresh_list[1:]:
    prev_start, prev_end = merged[-1]

    if prev_end >= start:
        merged[-1] = (prev_start, max(prev_end, end))
    else:
        merged.append((start, end))

def is_fresh(ingredient: int) -> bool:
    l = bisect.bisect_left(merged, ingredient, key=lambda i: i[0])

    if l == 0:
        return False
    
    start, end = merged[l - 1]

    return start <= ingredient <= end

out = sum([end - start + 1 for start, end in merged])
print(out)