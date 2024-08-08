use std::collections::HashSet;
use std::hash::Hash;

pub(crate) fn get_inputs_in_remote_not_in_db<'a, 'b, T:Hash+Eq>(inputs_in_remote: &'a [T], inputs_in_db: &'b [T])
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
  let inputs_in_remote = to_hash_set(inputs_in_remote);

  let res = inputs_in_remote.difference(&inputs_in_db)
    .map(|x| *x)
    .collect::<Vec<_>>();
  res
}

pub(crate) fn get_inputs_in_db_not_in_remote<'a, T:Hash+Eq>(inputs_in_remote: &'a [T], inputs_in_db: &'a [T])
                                                      -> Vec<&'a T>
{
  // use hash tables to avoid quadratic algorithm
  // todo(perf) use faster hasher. We don't need the security from SIP
  let to_hash_set = |x: &'a [T]| {
    x
      .iter()
      .collect::<HashSet<&'a T>>()
  };
  let inputs_in_db = to_hash_set(inputs_in_db);
  let inputs_in_remote = to_hash_set(inputs_in_remote);

  let res = inputs_in_db.difference(&inputs_in_remote)
    .map(|x| *x)
    .collect::<Vec<_>>();
  res
}
