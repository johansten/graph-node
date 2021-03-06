use futures::prelude::*;
use http::status::StatusCode;
use hyper::{Body, Response};
use serde::ser::*;
use serde_json;

use graph::components::server::GraphQLServerError;
use graph::data::query::QueryResult;
use graph_graphql::prelude::SerializableValue;

/// Future for HTTP responses to GraphQL query requests.
pub struct GraphQLResponse {
    result: Result<QueryResult, GraphQLServerError>,
}

impl GraphQLResponse {
    /// Creates a new GraphQLResponse future based on the result generated by
    /// running a query.
    pub fn new(result: Result<QueryResult, GraphQLServerError>) -> Self {
        GraphQLResponse { result }
    }

    fn status_code_from_result(&self) -> StatusCode {
        match self.result {
            Ok(ref result) => {
                if let Some(_) = result.errors {
                    StatusCode::BAD_REQUEST
                } else {
                    StatusCode::OK
                }
            }
            Err(ref e) => match e {
                &GraphQLServerError::ClientError(_) | &GraphQLServerError::QueryError(_) => {
                    StatusCode::BAD_REQUEST
                }
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            },
        }
    }
}

impl Serialize for GraphQLResponse {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self.result {
            Ok(ref result) => {
                let mut map = serializer.serialize_map(None)?;

                if let Some(ref data) = result.data {
                    map.serialize_entry("data", &SerializableValue(&data))?;
                }

                if let Some(ref errors) = result.errors {
                    map.serialize_entry("errors", errors)?;
                }

                map.end()
            }
            Err(ref e) => {
                let mut map = serializer.serialize_map(Some(1))?;
                let errors = vec![e];
                map.serialize_entry("errors", &errors)?;
                map.end()
            }
        }
    }
}

impl Future for GraphQLResponse {
    type Item = Response<Body>;
    type Error = GraphQLServerError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let status_code = self.status_code_from_result();
        let json =
            serde_json::to_string(self).expect("Failed to serialize GraphQL response to JSON");
        let response = Response::builder()
            .status(status_code)
            .header("Access-Control-Allow-Origin", "*")
            .header("Access-Control-Allow-Headers", "Content-Type")
            .body(Body::from(json))
            .unwrap();
        Ok(Async::Ready(response))
    }
}

#[cfg(test)]
mod tests {
    use super::GraphQLResponse;
    use futures::sync::oneshot;
    use graph::components::server::GraphQLServerError;
    use graph::prelude::*;
    use graphql_parser;
    use http::status::StatusCode;
    use std::collections::BTreeMap;
    use tokio_core::reactor::Core;

    use test_utils;

    #[test]
    fn generates_500_for_internal_errors() {
        let mut core = Core::new().unwrap();
        let future = GraphQLResponse::new(Err(GraphQLServerError::from("Some error")));
        let response = core.run(future).expect("Should generate a response");
        test_utils::assert_error_response(&mut core, response, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn generates_401_for_client_errors() {
        let mut core = Core::new().unwrap();
        let error = GraphQLServerError::ClientError(String::from("foo"));
        let future = GraphQLResponse::new(Err(error));
        let response = core.run(future).expect("Should generate a response");
        test_utils::assert_error_response(&mut core, response, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn generates_401_for_query_errors() {
        let mut core = Core::new().unwrap();
        let parse_error = graphql_parser::parse_query("<>?><").unwrap_err();
        let query_error = QueryError::from(parse_error);
        let future = GraphQLResponse::new(Err(GraphQLServerError::from(query_error)));
        let response = core.run(future).expect("Should generate a response");
        test_utils::assert_error_response(&mut core, response, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn generates_200_for_query_results() {
        let mut core = Core::new().unwrap();
        let data = graphql_parser::query::Value::Object(BTreeMap::new());
        let query_result = QueryResult::new(Some(data));
        let future = GraphQLResponse::new(Ok(query_result));
        let response = core.run(future).expect("Should generate a response");
        test_utils::assert_successful_response(&mut core, response);
    }

    #[test]
    fn generates_valid_json_for_an_empty_result() {
        let mut core = Core::new().unwrap();
        let data = graphql_parser::query::Value::Object(BTreeMap::new());
        let query_result = QueryResult::new(Some(data));
        let future = GraphQLResponse::new(Ok(query_result));
        let response = core.run(future).expect("Should generate a response");
        let data = test_utils::assert_successful_response(&mut core, response);
        assert!(data.is_empty());
    }

    #[test]
    fn generates_valid_json_when_canceled() {
        let mut core = Core::new().unwrap();
        let err = GraphQLServerError::Canceled(oneshot::Canceled);
        let future = GraphQLResponse::new(Err(err));
        let response = core.run(future).expect("Should generate a response");
        let errors = test_utils::assert_error_response(
            &mut core,
            response,
            StatusCode::INTERNAL_SERVER_ERROR,
        );
        assert_eq!(errors.len(), 1);

        let message = errors[0]
            .as_object()
            .expect("Cancellation error is not an object")
            .get("message")
            .expect("Error contains no message")
            .as_str()
            .expect("Error message is not a string");

        assert_eq!(message, "Query was canceled");
    }

    #[test]
    fn generates_valid_json_for_client_error() {
        let mut core = Core::new().unwrap();
        let err = GraphQLServerError::ClientError(String::from("Something went wrong"));
        let future = GraphQLResponse::new(Err(err));
        let response = core.run(future).expect("Should generate a response");
        let errors =
            test_utils::assert_error_response(&mut core, response, StatusCode::BAD_REQUEST);
        assert_eq!(errors.len(), 1);

        let message = errors[0]
            .as_object()
            .expect("Client error is not an object")
            .get("message")
            .expect("Error contains no message")
            .as_str()
            .expect("Error message is not a string");

        assert_eq!(message, "Something went wrong");
    }

    #[test]
    fn generates_valid_json_for_query_error() {
        let mut core = Core::new().unwrap();
        let parse_error =
            graphql_parser::parse_query("<><?").expect_err("Should fail parsing an invalid query");
        let query_error = QueryError::from(parse_error);
        let err = GraphQLServerError::QueryError(query_error);
        let future = GraphQLResponse::new(Err(err));
        let response = core.run(future).expect("Should generate a response");
        let errors =
            test_utils::assert_error_response(&mut core, response, StatusCode::BAD_REQUEST);
        assert_eq!(errors.len(), 1);

        let message = errors[0]
            .as_object()
            .expect("Query error is not an object")
            .get("message")
            .expect("Error contains no message")
            .as_str()
            .expect("Error message is not a string");

        assert_eq!(
            message,
            "Unexpected `unexpected character \
             \'<\'`\nExpected `{`, `query`, `mutation`, \
             `subscription` or `fragment`"
        );

        let locations = errors[0]
            .as_object()
            .expect("Query error is not an object")
            .get("locations")
            .expect("Query error contains not locations")
            .as_array()
            .expect("Query error \"locations\" field is not an array");

        let location = locations[0]
            .as_object()
            .expect("Query error location is not an object");

        let line = location
            .get("line")
            .expect("Query error location is missing a \"line\" field")
            .as_u64()
            .expect("Query error location \"line\" field is not a u64");

        assert_eq!(line, 1);

        let column = location
            .get("column")
            .expect("Query error location is missing a \"column\" field")
            .as_u64()
            .expect("Query error location \"column\" field is not a u64");

        assert_eq!(column, 1);
    }

    #[test]
    fn generates_valid_json_for_internal_error() {
        let mut core = Core::new().unwrap();
        let err = GraphQLServerError::InternalError(String::from("Something went wrong"));
        let future = GraphQLResponse::new(Err(err));
        let response = core.run(future).expect("Should generate a response");
        let errors = test_utils::assert_error_response(
            &mut core,
            response,
            StatusCode::INTERNAL_SERVER_ERROR,
        );
        assert_eq!(errors.len(), 1);

        let message = errors[0]
            .as_object()
            .expect("Client error is not an object")
            .get("message")
            .expect("Error contains no message")
            .as_str()
            .expect("Error message is not a string");

        assert_eq!(message, "Something went wrong");
    }
}
