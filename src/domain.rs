use uuid::Uuid;

#[derive(sqlx::Type)]
#[sqlx(transparent)]
pub struct UserId(Uuid);

#[derive(sqlx::Type)]
#[sqlx(transparent)]
pub struct UserEmail(String);

pub struct User {
    pub id: UserId,
    pub email: UserEmail,
}

impl User {}
