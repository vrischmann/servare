use std::io;

pub struct DashboardApplication {
    pub port: u16,
    pub pool: sqlx::PgPool,
}

impl DashboardApplication {
    pub async fn build_with_pool(pool: sqlx::PgPool) -> Result<DashboardApplication, io::Error> {
        Ok(DashboardApplication { port: 2000, pool })
    }
}
