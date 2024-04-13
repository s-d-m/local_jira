use std::collections::HashSet;
use std::hash::Hash;

pub(crate) fn get_inputs_not_in_db<'a, 'b, T:Hash+Eq>(inputs: &'a [T], inputs_in_db: &'b [T])
                                -> Vec<&'a T>
  where 'b: 'a
{
  // use hash tables to avoid quadratic algorithm
  // todo(perf) use faster hasher. We don't need the security from SIP
  let to_hash_set = |x: &'a [T]| {
    x
      .iter()
      .collect::<HashSet<&'a T>>()
  };
  let inputs_in_db = to_hash_set(inputs_in_db);
  let inputs = to_hash_set(inputs);

  let res = inputs.difference(&inputs_in_db)
    .map(|x| *x)
    .collect::<Vec<_>>();
  res
}
