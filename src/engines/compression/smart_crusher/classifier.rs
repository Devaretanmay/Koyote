use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArrayType {
    DictArray,
    StringArray,
    NumberArray,
    BoolArray,
    NestedArray,
    MixedArray,
    Empty,
}

pub fn classify_array(items: &[Value]) -> ArrayType {
    if items.is_empty() {
        return ArrayType::Empty;
    }

    let mut has_bool = false;
    let mut has_number = false;
    let mut has_string = false;
    let mut has_object = false;
    let mut has_array = false;
    let mut has_null = false;

    for item in items {
        match item {
            Value::Bool(_) => has_bool = true,
            Value::Number(_) => has_number = true,
            Value::String(_) => has_string = true,
            Value::Object(_) => has_object = true,
            Value::Array(_) => has_array = true,
            Value::Null => has_null = true,
        }
    }

    if has_bool && !has_number && !has_string && !has_object && !has_array && !has_null {
        return ArrayType::BoolArray;
    }

    if has_object && !has_bool && !has_number && !has_string && !has_array && !has_null {
        return ArrayType::DictArray;
    }

    if has_string && !has_bool && !has_number && !has_object && !has_array && !has_null {
        return ArrayType::StringArray;
    }

    if has_number && !has_bool && !has_string && !has_object && !has_array && !has_null {
        return ArrayType::NumberArray;
    }

    if has_array && !has_bool && !has_number && !has_string && !has_object && !has_null {
        return ArrayType::NestedArray;
    }

    ArrayType::MixedArray
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_array() {
        let items: Vec<Value> = vec![];
        assert_eq!(classify_array(&items), ArrayType::Empty);
    }

    #[test]
    fn pure_dict_array() {
        let items = vec![json!({"a": 1}), json!({"b": 2})];
        assert_eq!(classify_array(&items), ArrayType::DictArray);
    }

    #[test]
    fn pure_string_array() {
        let items = vec![json!("a"), json!("b"), json!("c")];
        assert_eq!(classify_array(&items), ArrayType::StringArray);
    }

    #[test]
    fn pure_number_array_int_and_float() {
        let items = vec![json!(1), json!(2.5), json!(3)];
        assert_eq!(classify_array(&items), ArrayType::NumberArray);
    }

    #[test]
    fn pure_bool_array() {
        let items = vec![json!(true), json!(false), json!(true)];
        assert_eq!(classify_array(&items), ArrayType::BoolArray);
    }

    #[test]
    fn nested_array() {
        let items = vec![json!([1, 2]), json!([3, 4])];
        assert_eq!(classify_array(&items), ArrayType::NestedArray);
    }

    #[test]
    fn mixed_dict_and_string_is_mixed() {
        let items = vec![json!({"a": 1}), json!("str")];
        assert_eq!(classify_array(&items), ArrayType::MixedArray);
    }

    #[test]
    fn bool_with_number_is_mixed_not_bool_or_number() {
        let items = vec![json!(true), json!(false), json!(1)];
        assert_eq!(classify_array(&items), ArrayType::MixedArray);
    }

    #[test]
    fn null_in_array_is_mixed() {
        let items = vec![json!({"a": 1}), json!(null)];
        assert_eq!(classify_array(&items), ArrayType::MixedArray);
    }
}
