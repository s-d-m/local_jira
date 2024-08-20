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


// jira seems to enclose some fields with quotes and others not, which is
// inconvenient
pub fn remove_surrounding_quotes(in_str: String) -> String {
  let substr = get_str_without_surrounding_quotes(in_str.as_str());
  if (substr.len() == in_str.len()) || substr.contains('"') {
    in_str
  } else {
    // todo(perf): check how to remove the chars direction from the string
    // without creating a new one
    // only remove surrounding quotes in there are none inside the
    // string. Otherwise, it is possible to accidentally change the
    // meaning of the string: e.g. the string
    // "John Doe" is nicer than "M. 23"
    // would get changed to
    // John Doe" is nicer than "M. 23
    // Unfortunately, there are no fully good solution here. For
    // the following string
    // "units are either "m/s" or "km/h" but never imperial units"
    // we ideally would remove the quotes at the beginning and end,
    // but how to tell that string apart than the example with John Doe?
    String::from(substr)
  }
}

pub fn get_str_without_surrounding_quotes(input: &str) -> &str {
  if input.starts_with('"') && input.ends_with('"') {
    let len = input.len();
    &input[1..(len - 1)]
  } else {
    input
  }
}