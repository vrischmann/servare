use uuid::Uuid;

pub struct UserID(Uuid);

pub struct UserEmail(String);

pub struct User {
    pub id: UserID,
    pub email: UserEmail,
}

impl User {}
