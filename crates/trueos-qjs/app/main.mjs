import { greet } from './util.mjs';
import { subHello } from './sub/rel.mjs';
import { make, add, square } from 'complex';

greet();
subHello();

const a = make(1, 2);
const b = make(3, 4);
const z = add(a, b);

print('z = ' + JSON.stringify(z));
print('square(z) = ' + JSON.stringify(square(z)));
