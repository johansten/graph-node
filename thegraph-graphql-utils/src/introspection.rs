use graphql_parser;
use graphql_parser::query as q;
use graphql_parser::schema as s;
use std::collections::HashMap;

use ast::query::object_value;
use ast::schema as sast;

const INTROSPECTION_SCHEMA: &'static str = "
scalar Boolean
scalar Float
scalar Int
scalar ID
scalar String

type Query {
  __schema: __Schema!
  __type(name: String!): __Type
}

type __Schema {
  types: [__Type!]!
  queryType: __Type!
  mutationType: __Type
  directives: [__Directive!]!
}

type __Type {
  kind: __TypeKind!
  name: String
  description: String

  # OBJECT and INTERFACE only
  fields(includeDeprecated: Boolean = false): [__Field!]

  # OBJECT only
  interfaces: [__Type!]

  # INTERFACE and UNION only
  possibleTypes: [__Type!]

  # ENUM only
  enumValues(includeDeprecated: Boolean = false): [__EnumValue!]

  # INPUT_OBJECT only
  inputFields: [__InputValue!]

  # NON_NULL and LIST only
  ofType: __Type
}

type __Field {
  name: String!
  description: String
  args: [__InputValue!]!
  type: __Type!
  isDeprecated: Boolean!
  deprecationReason: String
}

type __InputValue {
  name: String!
  description: String
  type: __Type!
  defaultValue: String
}

type __EnumValue {
  name: String!
  description: String
  isDeprecated: Boolean!
  deprecationReason: String
}

enum __TypeKind {
  SCALAR
  OBJECT
  INTERFACE
  UNION
  ENUM
  INPUT_OBJECT
  LIST
  NON_NULL
}

type __Directive {
  name: String!
  description: String
  locations: [__DirectiveLocation!]!
  args: [__InputValue!]!
}

enum __DirectiveLocation {
  QUERY
  MUTATION
  FIELD
  FRAGMENT_DEFINITION
  FRAGMENT_SPREAD
  INLINE_FRAGMENT
}";

pub fn introspection_schema() -> s::Document {
    graphql_parser::parse_schema(INTROSPECTION_SCHEMA).unwrap()
}

fn resolve_parent_type(
    schema: &s::Document,
    field: &'static str,
    parent_value: &Option<q::Value>,
) -> Option<(&q::Name, &s::TypeDefinition)> {
    parent_value
        .and_then(|value| match value {
            Some(q::Value::Object(values)) => Some(values),
            _ => None,
        })
        .and_then(|values| values.get(field))
        .and_then(|name| match sast::get_named_type(schema, name) {
            Some(typedef) => Some((name, typedef)),
            _ => None,
        })
        .unwrap_or(None)
}

pub fn resolve_object_value(
    schema: &s::Document,
    parent_value: &Option<q::Value>,
    field_name: &q::Name,
    type_name: &q::Name,
    object_type: &s::ObjectType,
    _arguments: &HashMap<&q::Name, q::Value>,
) -> q::Value {
    println!("Resolve object value: {}, {}", field_name, type_name);
    println!("  Parent value: {:#?}", parent_value);

    match (field_name.as_str(), type_name.as_str()) {
        (_, "__Schema") => schema_object(schema),
        ("queryType", "__Type") => query_type(schema),
        ("mutationType", "__Type") => q::Value::Null,
        ("type", "__Type") => {
            // TODO
            q::Value::Null
        }
        _ => unimplemented!(),
    }
}

pub fn resolve_object_values(
    schema: &s::Document,
    parent_value: &Option<q::Value>,
    field_name: &q::Name,
    type_name: &q::Name,
    object_type: &s::ObjectType,
    _arguments: &HashMap<&q::Name, q::Value>,
) -> q::Value {
    println!("Resolve object values: {}, {}", field_name, type_name);
    println!("  Parent value: {:#?}", parent_value);

    match (field_name.as_str(), type_name.as_str()) {
        ("types", "__Type") => schema_types(schema),
        ("fields", "__Field") => match resolve_parent_type(schema, parent_value, "name") {
            Some((name, s::TypeDefinition::Object(ot))) => field_objects(schema, name, &ot.fields),
            _ => q::Value::Null,
        },
        ("inputFields", "__InputValue") => q::Value::Null,
        ("interfaces", "__Type") => q::Value::Null,
        ("enumValues", "__EnumValue") => q::Value::Null,
        ("possibleTypes", "__Type") => q::Value::Null,
        ("args", "__InputValue") => {
            let parent_type = resolve_parent_type(schema, parent_value, "_parentTypeName");
            let field_name = match parent_value {
                q::Value::Object(ref values) => match values.get("name") {
                    Some(q::Value::String(ref name)) => Some(name),
                    _ => None,
                },
                _ => None,
            };

            match (parent_type_name, field_name) {
                (Some(parent_type_name), Some(field_name)) => {
                    match sast::get_named_type(schema, parent_type_name) {
                        Some(s::TypeDefinition::Object(ot)) => {
                            match sast::get_field_type(ot, field_name) {
                                Some(field) => input_values(schema, &field.arguments),
                                _ => q::Value::Null,
                            }
                        }
                        _ => q::Value::Null,
                    }
                }
                _ => q::Value::Null,
            }
        }
        _ => unimplemented!(),
    }
}

fn schema_object(schema: &s::Document) -> q::Value {
    object_value(vec![
        ("queryType", q::Value::Null),
        ("mutationType", q::Value::Null),
        ("types", q::Value::Null),
        ("directives", q::Value::Null),
        // ("queryType", query_type(schema)),
        // ("mutationType", q::Value::Null),
        // ("types", (schema_types(schema)),
        // ("directives", schema_directives(schema)),
    ])
}

fn query_type(schema: &s::Document) -> q::Value {
    sast::get_root_query_type(schema)
        .map(|t| object_type_object(schema, t))
        .expect("No Query type defined at the root of the GraphQL schema")
}

fn schema_types(schema: &s::Document) -> q::Value {
    q::Value::List(
        schema
            .definitions
            .iter()
            .filter_map(|d| match d {
                s::Definition::TypeDefinition(td) => Some(td),
                _ => None,
            })
            .map(|td| type_definition_object(schema, td))
            .filter(|td| td != &q::Value::Null)
            .collect(),
    )
}

fn schema_directives(schema: &s::Document) -> q::Value {
    q::Value::List(
        schema
            .definitions
            .iter()
            .filter_map(|d| match d {
                s::Definition::DirectiveDefinition(dd) => Some(dd),
                _ => None,
            })
            .map(|dd| directive_object(schema, dd))
            .collect(),
    )
}

fn type_object(schema: &s::Document, t: &s::Type) -> q::Value {
    match t {
        s::Type::NamedType(s) => named_type_object(schema, s),
        s::Type::ListType(ref inner) => list_type_object(schema, inner),
        s::Type::NonNullType(ref inner) => non_null_type_object(schema, inner),
    }
}

fn named_type_object(schema: &s::Document, name: &s::Name) -> q::Value {
    let named_type = sast::get_named_type(schema, name).expect(&format!(
        "Failed to resolve named type in GraphQL schema: {}",
        name
    ));

    type_definition_object(schema, named_type)
}

fn type_definition_object(schema: &s::Document, typedef: &s::TypeDefinition) -> q::Value {
    match typedef {
        s::TypeDefinition::Object(ot) => object_type_object(schema, ot),
        s::TypeDefinition::Enum(et) => enum_type_object(et),
        s::TypeDefinition::Scalar(st) => scalar_type_object(st),
        s::TypeDefinition::InputObject(iot) => input_object_type_object(schema, iot),
        s::TypeDefinition::Interface(it) => interface_type_object(schema, it),
        s::TypeDefinition::Union(ut) => union_type_object(schema, ut),
    }
}

fn list_type_object(schema: &s::Document, inner_type: &s::Type) -> q::Value {
    object_value(vec![
        ("kind", q::Value::Enum("LIST".to_string())),
        ("ofType", type_object(schema, inner_type)),
    ])
}

fn non_null_type_object(schema: &s::Document, inner_type: &s::Type) -> q::Value {
    object_value(vec![
        ("kind", q::Value::Enum("NON_NULL".to_string())),
        ("ofType", type_object(schema, inner_type)),
    ])
}

fn object_type_object(schema: &s::Document, object_type: &s::ObjectType) -> q::Value {
    object_value(vec![
        ("kind", q::Value::Enum("OBJECT".to_string())),
        ("name", q::Value::String(object_type.name.to_owned())),
        (
            "description",
            match object_type.description {
                Some(ref s) => q::Value::String(s.to_owned()),
                None => q::Value::Null,
            },
        ),
        ("fields", q::Value::Null),
        ("interfaces", q::Value::Null),
    ])
}

fn object_type_object_without_interfaces(
    schema: &s::Document,
    object_type: &s::ObjectType,
) -> q::Value {
    object_value(vec![
        ("kind", q::Value::Enum("OBJECT".to_string())),
        ("name", q::Value::String(object_type.name.to_owned())),
        (
            "description",
            match object_type.description {
                Some(ref s) => q::Value::String(s.to_owned()),
                None => q::Value::Null,
            },
        ),
        ("fields", q::Value::Null),
    ])
}

fn field_objects(
    schema: &s::Document,
    parent_type_name: &q::Name,
    fields: &Vec<s::Field>,
) -> q::Value {
    q::Value::List(
        fields
            .into_iter()
            .map(|field| field_object(schema, parent_type_name, field))
            .collect(),
    )
}

fn object_interfaces(schema: &s::Document, object_type: &s::ObjectType) -> q::Value {
    q::Value::List(
        object_type
            .implements_interfaces
            .iter()
            .map(|name| named_type_object(schema, name))
            .collect(),
    )
}

fn field_object(schema: &s::Document, parent_type_name: &q::Name, field: &s::Field) -> q::Value {
    object_value(vec![
        (
            "_parentTypeName",
            q::Value::String(parent_type_name.to_owned()),
        ),
        ("name", q::Value::String(field.name.to_owned())),
        (
            "description",
            match field.description {
                Some(ref s) => q::Value::String(s.to_owned()),
                None => q::Value::Null,
            },
        ),
        ("args", q::Value::Null),
        ("type", q::Value::Null),
        ("isDeprecated", q::Value::Boolean(false)),
        ("deprecationReason", q::Value::Null),
    ])
}

fn input_values(schema: &s::Document, input_values: &Vec<s::InputValue>) -> q::Value {
    q::Value::List(
        input_values
            .iter()
            .map(|value| input_value(schema, value))
            .collect(),
    )
}

fn input_value(schema: &s::Document, input_value: &s::InputValue) -> q::Value {
    object_value(vec![
        ("name", q::Value::String(input_value.name.to_owned())),
        (
            "description",
            match input_value.description {
                Some(ref s) => q::Value::String(s.to_owned()),
                None => q::Value::Null,
            },
        ),
        ("type", type_object(schema, &input_value.value_type)),
        (
            "defaultValue",
            match input_value.default_value {
                Some(ref v) => q::Value::String(format!("{:?}", v)),
                None => q::Value::Null,
            },
        ),
    ])
}

fn enum_type_object(enum_type: &s::EnumType) -> q::Value {
    object_value(vec![
        ("kind", q::Value::Enum("ENUM".to_string())),
        ("name", q::Value::String(enum_type.name.to_owned())),
        (
            "description",
            match enum_type.description {
                Some(ref s) => q::Value::String(s.to_owned()),
                None => q::Value::Null,
            },
        ),
        ("enumValues", q::Value::Null),
    ])
}

fn enum_values(enum_type: &s::EnumType) -> q::Value {
    q::Value::List(enum_type.values.iter().map(enum_value).collect())
}

fn enum_value(enum_value: &s::EnumValue) -> q::Value {
    object_value(vec![
        ("name", q::Value::String(enum_value.name.to_owned())),
        (
            "description",
            match enum_value.description {
                Some(ref s) => q::Value::String(s.to_owned()),
                None => q::Value::Null,
            },
        ),
        ("isDeprecated", q::Value::Boolean(false)),
        ("deprecationReason", q::Value::Null),
    ])
}

fn scalar_type_object(scalar_type: &s::ScalarType) -> q::Value {
    object_value(vec![
        ("name", q::Value::String(scalar_type.name.to_owned())),
        ("kind", q::Value::Enum("SCALAR".to_string())),
        (
            "description",
            match scalar_type.description {
                Some(ref s) => q::Value::String(s.to_owned()),
                None => q::Value::Null,
            },
        ),
        ("isDeprecated", q::Value::Boolean(false)),
        ("deprecationReason", q::Value::Null),
    ])
}

fn interface_type_object(schema: &s::Document, interface_type: &s::InterfaceType) -> q::Value {
    object_value(vec![
        ("name", q::Value::String(interface_type.name.to_owned())),
        ("kind", q::Value::Enum("INTERFACE".to_string())),
        (
            "description",
            match interface_type.description {
                Some(ref s) => q::Value::String(s.to_owned()),
                None => q::Value::Null,
            },
        ),
        ("fields", q::Value::Null),
        ("possibleTypes", q::Value::Null),
    ])
}

fn possible_types_for_interface(
    schema: &s::Document,
    interface_type: &s::InterfaceType,
) -> q::Value {
    q::Value::List(
        schema
            .definitions
            .iter()
            .filter_map(|d| match d {
                s::Definition::TypeDefinition(s::TypeDefinition::Object(ot)) => Some(ot),
                _ => None,
            })
            .filter_map(|ot| {
                ot.implements_interfaces
                    .iter()
                    .cloned()
                    .find(|name| name == &interface_type.name)
                    .map(|_| ot)
            })
            .map(|ot| object_type_object_without_interfaces(schema, ot))
            .collect(),
    )
}

fn input_object_type_object(
    schema: &s::Document,
    input_object_type: &s::InputObjectType,
) -> q::Value {
    object_value(vec![
        ("name", q::Value::String(input_object_type.name.to_owned())),
        ("kind", q::Value::Enum("INPUT_OBJECT".to_string())),
        (
            "description",
            match input_object_type.description {
                Some(ref s) => q::Value::String(s.to_owned()),
                None => q::Value::Null,
            },
        ),
        ("inputFields", q::Value::Null),
    ])
}

fn union_type_object(_schema: &s::Document, _union_object_type: &s::UnionType) -> q::Value {
    unimplemented!()
}

fn directive_object(schema: &s::Document, directive: &s::DirectiveDefinition) -> q::Value {
    object_value(vec![
        ("name", q::Value::String(directive.name.to_owned())),
        (
            "description",
            match directive.description {
                Some(ref s) => q::Value::String(s.to_owned()),
                None => q::Value::Null,
            },
        ),
        ("locations", directive_locations(directive)),
        ("args", input_values(schema, &directive.arguments)),
    ])
}

fn directive_locations(directive: &s::DirectiveDefinition) -> q::Value {
    q::Value::List(
        directive
            .locations
            .iter()
            .map(|location| location.as_str())
            .map(|name| q::Value::String(name.to_owned()))
            .collect(),
    )
}
