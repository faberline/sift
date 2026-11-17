//! OpenAPI belongs to the assembly boundary. These descriptions are never used
//! to copy or encode journal records. Golden tests pin the whole document.

macro_rules! schema_adapter {
    ($model:ty, $description:ty) => {
        impl utoipa::PartialSchema for $model {
            fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
                <$description as utoipa::PartialSchema>::schema()
            }
        }

        impl utoipa::ToSchema for $model {
            fn name() -> std::borrow::Cow<'static, str> {
                <$description as utoipa::ToSchema>::name()
            }

            fn schemas(
                schemas: &mut Vec<(
                    String,
                    utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>,
                )>,
            ) {
                <$description as utoipa::ToSchema>::schemas(schemas)
            }
        }
    };
}

mod append_result;
mod event;
mod log;
mod metric;
mod stored_event;
mod trace;
