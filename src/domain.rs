use uuid::Uuid;

#[derive(sqlx::Type)]
#[sqlx(transparent)]
pub struct UserID(Uuid);

#[derive(sqlx::Type)]
#[sqlx(transparent)]
pub struct UserEmail(String);

pub struct User {
    pub id: UserID,
    pub email: UserEmail,
}

impl User {}
