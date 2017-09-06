pub struct Inits<T: Clone, It: Iterator<Item=T>> {
    iter: It,
    items: Vec<T>,
}

impl<T: Clone, It: Iterator<Item=T>> Iterator for Inits<T, It> {
    type Item = Vec<T>;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
            .map(|x| {
                self.items.push(x);

                self.items.clone()
            })
    }
}

pub trait IntoInits<T: Clone> {
    type Iter: Iterator<Item=T>;

    fn inits(self) -> Inits<T, Self::Iter>;
}

impl<T: Clone, It: Iterator<Item=T>> IntoInits<T> for It {
    type Iter = Self;

    fn inits(self) -> Inits<T, It> {
        Inits {
            iter: self,
            items: vec![],
        }
    }
}
